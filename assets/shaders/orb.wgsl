#import bevy_pbr::forward_io::VertexOutput
#import bevy_pbr::mesh_view_bindings::{globals, view}

struct OrbMaterial {
    // Brings world space into the orb's own frame, so spinning turns the light inside the glass
    // rather than moving the glass itself.
    rotation: mat4x4<f32>,
    swirl: f32,
    drift: f32,
    activity: f32,
    radius: f32,
    reveal: f32,
    glow: f32,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> material: OrbMaterial;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var image_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var image_sampler: sampler;

const MARCH_STEPS: i32 = 96;
const FBM_OCTAVES: i32 = 2;
const FBM_NORMALISER: f32 = 0.75;
const EMISSION_GAIN: f32 = 1.05;
/// Scales a ribbon by the inverse of its width, so narrowing them does not dim the orb.
const RIBBON_BRIGHTNESS: f32 = 0.2;
/// Radius of the disc the image is shown on, as a fraction of the orb's radius.
const IMAGE_EXTENT: f32 = 0.66;
const IMAGE_BRIGHTNESS: f32 = 1.05;
/// How much of the ribbon light the fully materialised image blocks out.
const IMAGE_OCCLUSION: f32 = 0.94;
/// How much a full-strength voice adds to the light inside the orb, as a multiple of its resting level.
const GLOW_GAIN: f32 = 1.6;
const GLOW_CORE: f32 = 3.4;
const GLOW_HALO: f32 = 0.34;
const GLOW_TINT: vec3<f32> = vec3<f32>(0.50, 0.62, 1.0);

fn hash(point: vec3<f32>) -> f32 {
    var q = fract(point * vec3<f32>(0.1031, 0.1030, 0.0973));
    q += dot(q, q.yxz + 33.33);
    return fract((q.x + q.y) * q.z);
}

/// Shifted every frame, so the sampling pattern never holds still long enough to be seen as a texture.
fn interleaved_gradient_noise(pixel: vec2<f32>, frame: u32) -> f32 {
    let shifted = pixel + 5.588238 * f32(frame % 64u);
    return fract(52.9829189 * fract(dot(shifted, vec2<f32>(0.06711056, 0.00583715))));
}

fn value_noise(point: vec3<f32>) -> f32 {
    let cell = floor(point);
    let offset = fract(point);
    let weight = offset * offset * (3.0 - 2.0 * offset);
    let c000 = hash(cell + vec3<f32>(0.0, 0.0, 0.0));
    let c100 = hash(cell + vec3<f32>(1.0, 0.0, 0.0));
    let c010 = hash(cell + vec3<f32>(0.0, 1.0, 0.0));
    let c110 = hash(cell + vec3<f32>(1.0, 1.0, 0.0));
    let c001 = hash(cell + vec3<f32>(0.0, 0.0, 1.0));
    let c101 = hash(cell + vec3<f32>(1.0, 0.0, 1.0));
    let c011 = hash(cell + vec3<f32>(0.0, 1.0, 1.0));
    let c111 = hash(cell + vec3<f32>(1.0, 1.0, 1.0));
    let x00 = mix(c000, c100, weight.x);
    let x10 = mix(c010, c110, weight.x);
    let x01 = mix(c001, c101, weight.x);
    let x11 = mix(c011, c111, weight.x);
    return mix(mix(x00, x10, weight.y), mix(x01, x11, weight.y), weight.z);
}

fn fbm(point: vec3<f32>) -> f32 {
    var total = 0.0;
    var amplitude = 0.5;
    var sample_point = point;
    for (var octave = 0; octave < FBM_OCTAVES; octave += 1) {
        total += amplitude * value_noise(sample_point);
        sample_point = sample_point * 1.93 + vec3<f32>(13.1, 7.3, 5.9);
        amplitude *= 0.5;
    }
    return total / FBM_NORMALISER;
}

/// Interwoven neon: electric blue through violet and magenta into hot pink and orange.
fn neon(position: f32) -> vec3<f32> {
    // Weighted towards the cool half: three stops carry blue through violet, so the warm
    // accents stay accents.
    var stops = array<vec3<f32>, 6>(
        vec3<f32>(0.06, 0.38, 1.00),
        vec3<f32>(0.26, 0.22, 1.00),
        vec3<f32>(0.60, 0.14, 1.00),
        vec3<f32>(1.00, 0.10, 0.86),
        vec3<f32>(1.00, 0.26, 0.46),
        vec3<f32>(1.00, 0.55, 0.14),
    );
    let scaled = clamp(position, 0.0, 1.0) * 5.0;
    let lower = min(i32(floor(scaled)), 4);
    return mix(stops[lower], stops[lower + 1], scaled - f32(lower));
}

/// Shears the field around the vertical axis so the strands wind rather than drift in parallel.
fn twist(point: vec3<f32>, amount: f32) -> vec3<f32> {
    let angle = amount * point.y;
    let sine = sin(angle);
    let cosine = cos(angle);
    return vec3<f32>(
        cosine * point.x - sine * point.z,
        point.y,
        sine * point.x + cosine * point.z,
    );
}

/// Entry and exit distance along a ray for the unit sphere at the origin. Exit below entry means
/// the ray misses.
fn sphere_span(origin: vec3<f32>, direction: vec3<f32>, radius: f32) -> vec2<f32> {
    let along = dot(origin, direction);
    let outside = dot(origin, origin) - radius * radius;
    let discriminant = along * along - outside;
    if discriminant < 0.0 {
        return vec2<f32>(1.0, -1.0);
    }
    let root = sqrt(discriminant);
    return vec2<f32>(max(-along - root, 0.0), -along + root);
}

/// Position across the ribbon bands at a point: the fractional part says where within a ribbon
/// it falls, so slicing this into bands is what turns a noise cloud into separate strands.
fn ribbon_coordinate(point: vec3<f32>, radius: f32, activity: f32) -> f32 {
    let wound = twist(point, 2.6 - 1.5 * radius);
    let warp = vec3<f32>(
        value_noise(wound * 1.3 + vec3<f32>(0.0, material.drift, 0.0)),
        value_noise(wound * 1.3 + vec3<f32>(7.1, 3.4 - material.drift, 1.9)),
        value_noise(wound * 1.3 + vec3<f32>(2.7, 8.2, 5.5 + material.drift)),
    ) - vec3<f32>(0.5);
    let flow = wound + warp * (0.55 + 0.60 * activity);
    let field = fbm(flow * 1.25 + vec3<f32>(0.0, -material.swirl * 0.10, 0.0));

    return field * 4.5 + radius * 0.35 - material.swirl * 0.07;
}

/// Light emitted per unit length at a point inside the orb, in orb-local space where the surface
/// sits at radius 1.
fn emission_at(point: vec3<f32>, activity: f32) -> vec3<f32> {
    let radius = length(point);

    // The strands run out to just under the glass, and leave the middle clear for the core to burn
    // through.
    let shell = smoothstep(1.0, 0.97, radius) * smoothstep(0.16, 0.48, radius);
    let core = exp(-radius * radius * 15.0) * (0.18 + 3.1 * activity + GLOW_CORE * material.glow);
    if shell <= 0.001 {
        return vec3<f32>(1.0, 0.94, 0.88) * core;
    }

    let sliced = ribbon_coordinate(point, radius, activity);
    let across = fract(sliced);
    let sharpness = mix(5.0, 16.0, activity);
    let profile = max(1.0 - abs(across * 2.0 - 1.0), 0.0);
    // A hot line down the middle of each ribbon gives it a crisp spine instead of a soft blur.
    let ribbon = (pow(profile, sharpness) + 0.8 * pow(profile, sharpness * 3.0))
        * (sharpness + 1.0) * RIBBON_BRIGHTNESS;

    // Hue drifts smoothly along the same coordinate that cuts the ribbons, so neighbouring
    // strands stay related instead of clashing into grey where they overlap.
    let hue = fract(sliced * 0.34 + 0.52 + 0.03 * material.drift);
    let frost = mix(0.40, 0.03, activity);

    var emission = neon(hue) * ribbon * (0.90 + 1.90 * activity);
    emission += vec3<f32>(1.0) * ribbon * frost * 0.12;
    emission += neon(fract(hue + 0.12)) * frost * 0.05;
    return emission * shell + vec3<f32>(1.0, 0.94, 0.88) * core;
}

/// The image, on a disc facing the viewer through the middle of the orb. Its emission is in `rgb`,
/// and `a` is how much of the ribbon light behind it should be hidden.
///
/// It materialises out of the ribbons: the arrival order at each pixel comes from the same field the
/// ribbons are cut from, so the image condenses along ribbon spines first and fills in between them,
/// with a band of neon running just ahead of the solid picture.
fn image_layer(nearest: vec3<f32>, plane: vec2<f32>, activity: f32) -> vec4<f32> {
    let reveal = material.reveal;
    if reveal <= 0.0001 {
        return vec4<f32>(0.0);
    }
    let disc_distance = length(plane) / IMAGE_EXTENT;
    let disc = 1.0 - smoothstep(0.86, 1.0, disc_distance);
    if disc <= 0.0 {
        return vec4<f32>(0.0);
    }

    let local = (material.rotation * vec4<f32>(nearest, 1.0)).xyz / material.radius;
    let sliced = ribbon_coordinate(local, length(local), activity);
    let spine = 1.0 - abs(fract(sliced) * 2.0 - 1.0);
    let grain = value_noise(local * 6.0 + vec3<f32>(material.drift));
    let arrival = 0.6 * (1.0 - spine) + 0.3 * grain + 0.1 * disc_distance;
    // Runs a little past both ends of the range, so zero is fully hidden and one fully shown.
    let front = reveal * 1.25 - 0.12;
    let solid = smoothstep(arrival - 0.07, arrival + 0.07, front);
    let leading_edge = smoothstep(arrival - 0.16, arrival, front) * (1.0 - solid);

    let shimmer = vec2<f32>(
        value_noise(local * 3.0 + vec3<f32>(material.drift)),
        value_noise(local * 3.0 + vec3<f32>(4.0, 0.0, 0.0) - vec3<f32>(material.drift)),
    ) - vec2<f32>(0.5);
    let uv = plane / IMAGE_EXTENT * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5) + shimmer * (1.0 - solid) * 0.06;
    let picture = textureSampleLevel(image_texture, image_sampler, uv, 0.0).rgb;

    let hue = fract(sliced * 0.34 + 0.52 + 0.03 * material.drift);
    let emission = (picture * IMAGE_BRIGHTNESS * solid + neon(hue) * leading_edge * 2.2) * disc;
    return vec4<f32>(emission, solid * disc);
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let camera = view.world_position;
    let ray = normalize(in.world_position.xyz - camera);
    let activity = material.activity;

    // How far the ray passes from the centre, as a fraction of the radius: 1 is the silhouette.
    // Working from this instead of a hit test lets the edge fade over a couple of pixels rather
    // than cutting off, and lets the bounding margin carry a faint halo past it.
    let along = dot(camera, ray);
    let closest = sqrt(max(dot(camera, camera) - along * along, 0.0)) / material.radius;
    let edge_width = max(fwidth(closest) * 4.0, 0.012);
    let inside = 1.0 - smoothstep(1.0 - edge_width, 1.0 + edge_width, closest);

    var accumulated = vec3<f32>(0.0);
    let span = sphere_span(camera, ray, material.radius);
    if span.y > span.x {
        let step_size = (span.y - span.x) / f32(MARCH_STEPS);
        // Offsetting the first step per pixel trades banding for noise. Interleaved gradient noise
        // spreads that offset evenly across neighbouring pixels, and changing it every frame lets the
        // eye average it away.
        var travelled = span.x + step_size * interleaved_gradient_noise(in.position.xy, globals.frame_count);
        for (var step = 0; step < MARCH_STEPS; step += 1) {
            let world = camera + ray * travelled;
            let local = (material.rotation * vec4<f32>(world, 1.0)).xyz / material.radius;
            accumulated += emission_at(local, activity);
            travelled += step_size;
        }
        accumulated *= step_size * EMISSION_GAIN * (1.0 + GLOW_GAIN * material.glow);

        let nearest = camera - ray * along;
        let plane = (view.view_from_world * vec4<f32>(nearest, 0.0)).xy / material.radius;
        let image = image_layer(nearest, plane, activity);
        accumulated = accumulated * (1.0 - IMAGE_OCCLUSION * image.a) + image.rgb;

        let normal = normalize(camera + ray * span.x);
        let reflected = reflect(ray, normal);
        // Fixed to the view rather than the orb, so highlights stay put while the light inside turns.
        let key_light = normalize((view.world_from_view * vec4<f32>(-0.42, 0.58, 0.70, 0.0)).xyz);
        let fill_light = normalize((view.world_from_view * vec4<f32>(0.55, -0.50, 0.67, 0.0)).xyz);
        let key_alignment = max(dot(reflected, key_light), 0.0);
        let specular = pow(key_alignment, 260.0) * 1.1
            + pow(key_alignment, 22.0) * 0.05
            + pow(max(dot(reflected, fill_light), 0.0), 120.0) * 0.25;
        accumulated += vec3<f32>(0.96, 0.97, 1.0) * specular;

        // A sphere of glass focuses light into a faint crescent just inside the edge, on the side
        // away from the highlight.
        let view_normal = (view.view_from_world * vec4<f32>(normal, 0.0)).xyz;
        let away_from_key = dot(normalize(view_normal.xy + vec2<f32>(1e-5)), normalize(vec2<f32>(0.42, -0.58)));
        let crescent = smoothstep(0.74, 0.95, closest) * (1.0 - smoothstep(0.95, 1.0, closest));
        let caustic = crescent * smoothstep(0.25, 1.0, away_from_key) * mix(0.018, 0.045, activity);
        accumulated += vec3<f32>(0.62, 0.66, 1.0) * caustic;
    }

    // Glass only shows where it is seen edge on. This is deliberately faint: display encoding lifts
    // small linear values a long way, so anything brighter reads as a solid grey shell.
    let rim = pow(clamp(closest, 0.0, 1.0), 40.0) * mix(0.070, 0.045, activity);
    accumulated += vec3<f32>(0.72, 0.82, 1.0) * rim;
    accumulated *= inside;

    let beyond = max(closest - 1.0, 0.0);
    let halo = exp(-beyond * 30.0) * (1.0 - inside) * mix(0.030, 0.022, activity);
    accumulated += vec3<f32>(0.55, 0.62, 1.0) * halo;

    // A voice lights the air around the orb as well as the inside: a wide aura that reaches well past
    // the glass, plus a lift at the rim so the silhouette brightens with it.
    // The aura has to reach zero before the bounding mesh ends, or the mesh shows as a faint disc.
    let reach = 1.0 - smoothstep(0.0, 0.34, beyond);
    let aura = exp(-beyond * 9.0) * reach * (1.0 - inside) + pow(clamp(closest, 0.0, 1.0), 6.0) * inside * 0.35;
    accumulated += GLOW_TINT * aura * GLOW_HALO * material.glow;

    // Bevy's additive mode is a premultiplied blend that only adds when alpha is zero; any other
    // alpha paints the orb over whatever is behind it.
    return vec4<f32>(accumulated, 0.0);
}
