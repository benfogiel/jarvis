use bevy::log::error;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};

/// A sound has to be this many times louder than the room to count as a voice.
const VOICE_OVER_NOISE: f32 = 3.5;
/// Keeps a silent room from making every rustle register as a voice.
const QUIETEST_NOISE_FLOOR: f32 = 0.002;
const NOISE_FLOOR_RISE_PER_SECOND: f32 = 0.15;
const NOISE_FLOOR_FALL_PER_SECOND: f32 = 4.0;
/// How long a sound has to last before it counts as the start of speech, which rules out clicks and
/// knocks.
const ONSET_SECONDS: f32 = 0.12;
/// Audio kept from before the onset, so the first syllable is not clipped.
const LEAD_IN_SECONDS: f32 = 0.35;
const END_OF_SPEECH_SILENCE_SECONDS: f32 = 0.9;
const LONGEST_UTTERANCE_SECONDS: f32 = 30.0;
/// The span above the room's noise that maps onto loudness from zero to one.
const LOUDNESS_DECIBELS: (f32, f32) = (8.0, 34.0);

pub struct Microphone {
    /// Capture stops when the stream is dropped.
    _stream: cpal::Stream,
    chunks: Receiver<Vec<f32>>,
    sample_rate: u32,
    noise_floor: f32,
}

impl Microphone {
    pub fn open() -> Result<Self, String> {
        let device = cpal::default_host().default_input_device().ok_or("no microphone found")?;
        let supported = device.default_input_config().map_err(|error| error.to_string())?;
        let config = supported.config();
        let (sender, chunks) = mpsc::channel();
        let stream = match supported.sample_format() {
            SampleFormat::F32 => capture::<f32>(&device, &config, sender),
            SampleFormat::I16 => capture::<i16>(&device, &config, sender),
            SampleFormat::U16 => capture::<u16>(&device, &config, sender),
            other => Err(format!("unsupported microphone sample format {other:?}")),
        }?;
        stream.play().map_err(|error| error.to_string())?;
        Ok(Self {
            _stream: stream,
            chunks,
            sample_rate: config.sample_rate,
            noise_floor: QUIETEST_NOISE_FLOOR,
        })
    }

    /// Blocks until someone has spoken and then fallen silent, and returns what they said as mono
    /// samples at `output_rate`. `on_loudness` is called for every chunk of audio heard on the way.
    pub fn next_utterance(
        &mut self,
        output_rate: u32,
        mut on_loudness: impl FnMut(f32),
    ) -> Result<Vec<f32>, String> {
        let rate = self.sample_rate as f32;
        let mut lead_in = VecDeque::new();
        let mut utterance = Vec::new();
        let mut onset_seconds = 0.0;
        let mut trailing_silence_seconds = 0.0;
        let mut speaking = false;

        loop {
            let chunk = self.chunks.recv().map_err(|_| "the microphone stream closed")?;
            if chunk.is_empty() {
                continue;
            }
            let seconds = chunk.len() as f32 / rate;
            let level = root_mean_square(&chunk);
            let voiced = level > self.noise_floor * VOICE_OVER_NOISE;
            on_loudness(self.loudness(level));

            if speaking {
                utterance.extend_from_slice(&chunk);
                trailing_silence_seconds = if voiced { 0.0 } else { trailing_silence_seconds + seconds };
                if trailing_silence_seconds >= END_OF_SPEECH_SILENCE_SECONDS
                    || utterance.len() as f32 / rate >= LONGEST_UTTERANCE_SECONDS
                {
                    on_loudness(0.0);
                    return Ok(resample(&utterance, self.sample_rate, output_rate));
                }
                continue;
            }

            self.track_noise_floor(level, seconds);
            lead_in.extend(chunk);
            let surplus = lead_in.len().saturating_sub((LEAD_IN_SECONDS * rate) as usize);
            lead_in.drain(..surplus);
            onset_seconds = if voiced { onset_seconds + seconds } else { 0.0 };
            if onset_seconds >= ONSET_SECONDS {
                speaking = true;
                utterance.extend(lead_in.drain(..));
            }
        }
    }

    /// Throws away whatever was captured while nobody was listening, such as the orb's own voice.
    pub fn discard_pending(&self) {
        while self.chunks.try_recv().is_ok() {}
    }

    fn loudness(&self, level: f32) -> f32 {
        let decibels_over_noise = 20.0 * (level.max(f32::MIN_POSITIVE) / self.noise_floor).log10();
        let (quiet, loud) = LOUDNESS_DECIBELS;
        ((decibels_over_noise - quiet) / (loud - quiet)).clamp(0.0, 1.0)
    }

    /// Follows the room down quickly and up slowly, so a voice does not drag the floor up after it.
    fn track_noise_floor(&mut self, level: f32, seconds: f32) {
        let per_second = if level < self.noise_floor {
            NOISE_FLOOR_FALL_PER_SECOND
        } else {
            NOISE_FLOOR_RISE_PER_SECOND
        };
        self.noise_floor += (level - self.noise_floor) * (per_second * seconds).min(1.0);
        self.noise_floor = self.noise_floor.max(QUIETEST_NOISE_FLOOR);
    }
}

fn capture<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    sender: Sender<Vec<f32>>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let channels = config.channels as usize;
    device
        .build_input_stream(
            config,
            move |interleaved: &[T], _: &cpal::InputCallbackInfo| {
                let mono = interleaved
                    .chunks(channels)
                    .map(|frame| {
                        frame.iter().map(|sample| sample.to_sample::<f32>()).sum::<f32>() / channels as f32
                    })
                    .collect();
                let _ = sender.send(mono);
            },
            |error| error!("microphone stream failed: {error}"),
            None,
        )
        .map_err(|error| error.to_string())
}

fn root_mean_square(samples: &[f32]) -> f32 {
    (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32).sqrt()
}

/// Averages each run of input samples that lands on one output sample, which is enough of a low-pass
/// filter for speech.
fn resample(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate {
        return samples.to_vec();
    }
    let step = from_rate as f64 / to_rate as f64;
    let length = (samples.len() as f64 / step) as usize;
    (0..length)
        .map(|index| {
            let start = (index as f64 * step) as usize;
            let end = (((index + 1) as f64 * step) as usize).clamp(start + 1, samples.len());
            samples[start..end].iter().sum::<f32>() / (end - start) as f32
        })
        .collect()
}
