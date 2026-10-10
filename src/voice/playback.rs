use bevy::log::error;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::Duration;

/// The callback reports the end when it hands over the last sample, which is a little before the
/// device has finished playing it.
const DEVICE_DRAIN: Duration = Duration::from_millis(200);

/// Plays mono samples on the default output device, and returns once they have been heard.
pub fn play(samples: Vec<f32>, sample_rate: u32) -> Result<(), String> {
    let device = cpal::default_host().default_output_device().ok_or("no audio output found")?;
    let supported = device.default_output_config().map_err(|error| error.to_string())?;
    let config = supported.config();
    let (finished, done) = mpsc::channel();
    let stream = match supported.sample_format() {
        SampleFormat::F32 => render::<f32>(&device, &config, samples, sample_rate, finished),
        SampleFormat::I16 => render::<i16>(&device, &config, samples, sample_rate, finished),
        SampleFormat::U16 => render::<u16>(&device, &config, samples, sample_rate, finished),
        other => Err(format!("unsupported output sample format {other:?}")),
    }?;
    stream.play().map_err(|error| error.to_string())?;
    done.recv().map_err(|_| "the output stream closed before playback finished")?;
    thread::sleep(DEVICE_DRAIN);
    Ok(())
}

fn render<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: Vec<f32>,
    sample_rate: u32,
    finished: Sender<()>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let step = sample_rate as f64 / config.sample_rate as f64;
    let mut position = 0.0_f64;
    device
        .build_output_stream(
            config,
            move |output: &mut [T], _: &cpal::OutputCallbackInfo| {
                for frame in output.chunks_mut(channels) {
                    let index = position as usize;
                    let value = match (samples.get(index), samples.get(index + 1)) {
                        (Some(here), Some(next)) => here + (next - here) * position.fract() as f32,
                        (Some(last), None) => *last,
                        _ => 0.0,
                    };
                    frame.fill(T::from_sample(value));
                    position += step;
                }
                if position as usize >= samples.len() {
                    let _ = finished.send(());
                }
            },
            |error| error!("output stream failed: {error}"),
            None,
        )
        .map_err(|error| error.to_string())
}
