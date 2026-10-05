
// Terrain water, concatenated after `material.wgsl` (whose `material_output`
// hands every `KIND_WATER` fragment to `water_output`) wherever that file is.
//
// Roblox documents what its water properties do (`WaterWaveSize` "from 1
// (large) to 0 (none)", `WaterWaveSpeed` "from 100 (turbulent) to 0 (still)",
// `WaterReflectance`, `WaterTransparency`, `WaterColor`) but publishes no
// shader, so everything below is this renderer's own model, tuned by eye
// against captures of Roblox water: a lit, scattering turquoise body that
// fades in over the depth of water the eye looks through, fine irregular
// ripples tilting the normal (no displaced geometry), screen-space
// reflections of the opaque scene falling back to the sky probe, and sun
// glints on the ripples.
//
// `lighting.quality` carries the two quality knobs (see `crate::quality`):
// z turns the ripples on, w the screen-space reflections.

const WATER_ROUGHNESS: f32 = 0.05;
// Studs of water that dim light to 1/e at `WaterTransparency` 1; the place's
// transparency scales it (0.3, the default, gives 12 studs).
const WATER_CLEAR_STUDS: f32 = 40.0;
// `camera::NEAR_PLANE`, which a unit test holds this to.
const CAMERA_NEAR: f32 = 0.05;
// How far the strongest waves (size 1) tilt the normal.
const WATER_TILT: f32 = 0.35;
// The ripple octaves: wavelength in studs, relative slope, and how many laps
// of its looping path each one runs per wave cycle.
const WATER_OCTAVES: array<vec3<f32>, 3> = array<vec3<f32>, 3>(
    vec3<f32>(17.0, 0.55, 1.0),
    vec3<f32>(6.3, 0.8, 2.0),
    vec3<f32>(2.3, 1.0, 3.0),
);
// An octave is gone by this many of its own wavelengths away (fading over the
// second half): past that a pixel covers several ripples and they only alias
// into shimmer.
const WATER_OCTAVE_REACH: f32 = 70.0;
// The body: `WaterColor`'s hue lit like a matte surface facing up, at this
// fraction of the brightness its strongest channel would give at full
// strength. A square root of that channel keeps a darker `WaterColor` darker
// without letting Roblox's dark default (12, 84, 92) read as navy.
const WATER_BODY_GAIN: f32 = 0.85;
// The sun glint: Blinn-Phong exponent (tight enough that only crests
// turned just right catch it) and its strength over the sun's own colour.
const WATER_GLINT_SHININESS: f32 = 6000.0;
const WATER_GLINT_STRENGTH: f32 = 6.0;
// Screen-space reflection march: steps, the first step's length in studs and
// how much longer each step is than the last (24 steps reach ~400 studs).
const WATER_SSR_STEPS: i32 = 24;
const WATER_SSR_FIRST: f32 = 1.0;
const WATER_SSR_GROWTH: f32 = 1.2;
const WATER_SSR_REFINE: i32 = 5;
// How close to the frame's edge a reflection hit fades toward the probe.
const WATER_SSR_EDGE: f32 = 0.08;
// How much of the ripples' tilt the scene's reflection keeps (see
// `water_output`).
const WATER_SSR_RIPPLE: f32 = 0.35;

/// How far from the eye a depth value is, in studs along the view: infinite
/// reversed-Z puts depth at `CAMERA_NEAR / distance`, an orthographic frame (a
/// negative `uniforms.viewport.w`, see `pipeline::intersection_depth`) spreads
/// it linearly over its span. Only differences of this are meaningful.
fn water_view_distance(depth: f32) -> f32 {
    let mode = uniforms.viewport.w;
    if mode < 0.0 {
        return -depth * -mode;
    }
    return CAMERA_NEAR / max(depth, 1.0e-9);
}

/// How many studs of water lie between a water fragment and the opaque scene
/// behind it. Nothing behind at all reads as bottomless.
fn water_depth(frag: vec4<f32>) -> f32 {
    let scene = scene_depth(vec2<i32>(frag.xy));
    if scene <= 0.0 {
        return 1.0e4;
    }
    let mode = uniforms.viewport.w;
    if mode < 0.0 {
        return max((frag.z - scene) * -mode, 0.0);
    }
    return max(CAMERA_NEAR / scene - CAMERA_NEAR / frag.z, 0.0);
}

fn water_hash(cell: vec2<i32>) -> f32 {
    var h = (bitcast<u32>(cell.x) * 0x8da6b343u) ^ (bitcast<u32>(cell.y) * 0xd8163841u);
    h = (h ^ (h >> 16u)) * 0x7feb352du;
    h = (h ^ (h >> 15u)) * 0x846ca68bu;
    h = h ^ (h >> 16u);
    return f32(h) * (1.0 / 4294967295.0);
}

/// Value noise's gradient at `p` (in cells), from the quintic blend's own
/// derivative: a hash per lattice point and no texture, so it never tiles.
fn water_noise_gradient(p: vec2<f32>) -> vec2<f32> {
    let cell = vec2<i32>(floor(p));
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let du = 30.0 * f * f * (f * (f - 2.0) + 1.0);
    let a = water_hash(cell);
    let b = water_hash(cell + vec2<i32>(1, 0));
    let c = water_hash(cell + vec2<i32>(0, 1));
    let d = water_hash(cell + vec2<i32>(1, 1));
    let k = a - b - c + d;
    return du * vec2<f32>(b - a + k * u.y, c - a + k * u.x);
}

/// The slope of rippling water at `xz`: each octave is two layers of value
/// noise, on a lattice rotated off the world axes (so no octave lines up with
/// the voxel grid or another octave), each layer sliding round a small closed
/// loop. A closed path is what lets noise, which never repeats in space,
/// still come back to exactly where it started when the wave cycle ends.
fn water_slope(xz: vec2<f32>, time: f32, distance: f32) -> vec2<f32> {
    var slope = vec2<f32>(0.0);
    var angle = 0.6;
    for (var i = 0; i < 3; i = i + 1) {
        let octave = WATER_OCTAVES[i];
        let reach = saturate(2.0 - distance / (octave.x * WATER_OCTAVE_REACH * 0.5));
        let turn = mat2x2<f32>(cos(angle), sin(angle), -sin(angle), cos(angle));
        let p = (turn * xz) / octave.x;
        let lap = time * octave.z;
        let a = p + 0.45 * vec2<f32>(cos(lap), sin(lap));
        let b = p.yx * vec2<f32>(1.0, -1.0) + vec2<f32>(17.3, 5.1)
            + 0.45 * vec2<f32>(sin(lap + 2.0), cos(lap + 2.0));
        let gb = water_noise_gradient(b);
        let g = water_noise_gradient(a) + vec2<f32>(-gb.y, gb.x);
        // Back into world axes; d/dx of noise(x / wavelength) carries 1 / wavelength,
        // which the octave weights already stand in for.
        slope += (transpose(turn) * g) * (octave.y * reach);
        angle += 2.1;
    }
    return slope;
}

/// The normal water is shaded about before its ripples. The mesh's slivers
/// where the surface meets a shore or the edge of the map lean every which
/// way; shaded with their own normals they catch the sky edge-on and read as
/// dark seams. A water surface is level, so anything not facing well down
/// (which is the surface seen from under it) is shaded as level.
fn water_base_normal(geometric: vec3<f32>) -> vec3<f32> {
    let normal = normalize(geometric);
    return select(normal, vec3<f32>(0.0, 1.0, 0.0), normal.y > -0.7);
}

/// The surface normal of rippling water at `input`, whose studs-per-tile
/// field carries the waves: whole wave cycles per shimmer period in its
/// integer part, height (0 to 1) in its fraction (see `renderer::terrain`).
/// Only upward-facing water ripples; a wall of water keeps its own normal.
fn water_normal(input: MaterialInput, distance: f32) -> vec3<f32> {
    let packed = input.studs_per_tile;
    let cycles = floor(packed);
    let height = fract(packed) / 0.999;
    let normal = water_base_normal(input.world_normal);
    let up = max(normal.y, 0.0);
    if height <= 0.0 || up <= 0.0 || lighting.quality.z < 0.5 {
        return normal;
    }
    let time = 6.2831855 * uniforms.viewport.z * cycles;
    let tilt = water_slope(input.world_position.xz, time, distance) * (height * WATER_TILT * up);
    return normalize(normal + vec3<f32>(-tilt.x, 0.0, -tilt.y));
}

/// `WaterColor` as the colour the body scatters: its hue, at a brightness
/// lifted from its strongest channel (see `WATER_BODY_GAIN`).
fn water_scatter(albedo: vec3<f32>) -> vec3<f32> {
    let peak = max(max(albedo.r, albedo.g), max(albedo.b, 1.0e-4));
    return albedo / peak * sqrt(peak) * WATER_BODY_GAIN;
}

/// The light a point of the water body is lit by, before exposure and fog:
/// the two lamps and the ambient terms on a surface facing straight up,
/// whatever the ripples do, since the light scatters out of the water under
/// them rather than off them. Returns the sun's visibility in `w`.
fn water_light(position: vec3<f32>) -> vec4<f32> {
    let up = vec3<f32>(0.0, 1.0, 0.0);
    let facing = normalize(lighting.sun_direction.xyz).y;
    let lamp = lighting.shadow_lamp.x;
    let visibility = lamp_visibility(position, up, select(-facing, facing, lamp > 0.0));
    let sun_lit = select(1.0, visibility, lamp > 0.5);
    let fill_lit = select(1.0, visibility, lamp < -0.5);
    let light = lighting.sun_color.rgb * saturate(facing) * sun_lit
        + lighting.fill_color.rgb * saturate(-facing) * fill_lit
        + lighting.ambient.rgb
        + sky_ambient(up);
    return vec4<f32>(light, sun_lit);
}

/// The opaque scene reflected along `direction` from `origin`, marched in
/// world space against the scene's depth buffer, with how much of it was
/// found in `w` (0 for a miss, fading toward the frame's edges and the far
/// end of the march). Reads with `textureLoad` only, since it loops.
fn water_reflection(origin: vec3<f32>, direction: vec3<f32>) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(refraction_source));
    var step = WATER_SSR_FIRST;
    var last = 0.0;
    var travelled = 0.0;
    for (var i = 0; i < WATER_SSR_STEPS; i = i + 1) {
        travelled += step;
        let clip = uniforms.view_projection * vec4<f32>(origin + direction * travelled, 1.0);
        if clip.w <= 0.0 {
            break;
        }
        let ndc = clip.xyz / clip.w;
        let uv = vec2<f32>(ndc.x, -ndc.y) * 0.5 + 0.5;
        if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
            break;
        }
        let scene = scene_depth(vec2<i32>(uv * size));
        let behind = water_view_distance(ndc.z) - water_view_distance(scene);
        // In front of the scene, or over the sky: keep going. Behind it by
        // more than a step's width is a ray passing behind something thin,
        // which reflects nothing from it.
        if scene > 0.0 && behind > 0.0 && behind < step * 2.0 + 1.0 {
            // Bisect between the last free point and this one.
            var near = last;
            var far = travelled;
            var hit = uv;
            for (var j = 0; j < WATER_SSR_REFINE; j = j + 1) {
                let mid = 0.5 * (near + far);
                let c = uniforms.view_projection * vec4<f32>(origin + direction * mid, 1.0);
                let n = c.xyz / c.w;
                let u = clamp(vec2<f32>(n.x, -n.y) * 0.5 + 0.5, vec2<f32>(0.0), vec2<f32>(1.0));
                let s = scene_depth(vec2<i32>(u * (size - 1.0)));
                if s > 0.0 && water_view_distance(n.z) > water_view_distance(s) {
                    far = mid;
                    hit = u;
                } else {
                    near = mid;
                }
            }
            let texel = clamp(vec2<i32>(hit * size), vec2<i32>(0), vec2<i32>(size) - vec2<i32>(1));
            let color = textureLoad(refraction_source, texel, 0).rgb;
            let edge = min(min(hit.x, 1.0 - hit.x), min(hit.y, 1.0 - hit.y));
            let found = smoothstep(0.0, WATER_SSR_EDGE, edge)
                * (1.0 - smoothstep(0.7, 1.0, f32(i) / f32(WATER_SSR_STEPS)));
            return vec4<f32>(color, found);
        }
        last = travelled;
        step *= WATER_SSR_GROWTH;
    }
    return vec4<f32>(0.0);
}

/// What a water fragment writes. With the opaque scene's copy bound, the
/// pixel is replaced outright: the bed seen through the water (bent by the
/// ripples, fading per channel into the body over the depth crossed), the
/// reflection over it by Fresnel, and the glint. Without one, the water
/// blends over what is behind it the ordinary way.
fn water_output(input: MaterialInput, alpha: f32, frag: vec4<f32>) -> vec4<f32> {
    let offset = lighting.camera.xyz - input.world_position;
    let distance = length(offset);
    let to_eye = offset / max(distance, 1.0e-4);
    let normal = water_normal(input, distance);
    let exposure = lighting.tuning.x;

    let lit = water_light(input.world_position);
    let scatter = water_scatter(input.albedo);
    let body = scatter * lit.rgb * exposure;

    // Fresnel, scaled by `WaterReflectance`: a mirror at a glance, the depth
    // face-on.
    let facing = max(dot(normal, to_eye), 0.0);
    let fresnel = saturate(input.reflectance) * (0.02 + 0.98 * pow(1.0 - facing, 5.0));

    // A ripple can turn the reflected ray below the surface; mirror it back
    // up rather than reflect the bed.
    let base = water_base_normal(input.world_normal);
    var reflected = reflect(-to_eye, normal);
    if base.y > 0.0 {
        reflected.y = abs(reflected.y);
    }
    let sky = env_sample(reflected, env_level(WATER_ROUGHNESS)) * exposure;
    var mirror = sky;
    let source = vec2<i32>(textureDimensions(refraction_source));
    if lighting.quality.w > 0.5 && source.x > 1 {
        // The scene is reflected about a calmer normal than the sky is: with
        // the full tilt its image shatters into specks, where real water keeps
        // a wobbling but readable mirror image of a shore.
        var toward = reflect(-to_eye, normalize(mix(base, normal, WATER_SSR_RIPPLE)));
        if base.y > 0.0 {
            toward.y = abs(toward.y);
        }
        let scene = water_reflection(input.world_position, toward);
        mirror = mix(sky, scene.rgb, scene.w);
    }

    let light = normalize(lighting.sun_direction.xyz);
    let half_vector = normalize(light + to_eye);
    let glint = lighting.sun_color.rgb * exposure * WATER_GLINT_STRENGTH * lit.w
        * saturate(light.y * 4.0)
        * exp2(WATER_GLINT_SHININESS * (dot(normal, half_vector) - 1.0));

    if alpha >= 1.0 || source.x <= 1 {
        let color = mix(body, mirror, fresnel) + glint;
        return vec4<f32>(fade(color, to_eye, distance), mix(alpha, 1.0, fresnel));
    }

    // The ripples bend what is seen through them, as the mapped normal bends
    // what is behind a pane of glass.
    let clip_normal = uniforms.view_projection * vec4<f32>(normal - water_base_normal(input.world_normal), 0.0);
    let bent = frag.xy / uniforms.viewport.xy
        + vec2<f32>(clip_normal.x, -clip_normal.y) * GLASS_REFRACTION;
    let uv = clamp(bent, vec2<f32>(0.0), vec2<f32>(1.0));
    let texel = clamp(vec2<i32>(uv * vec2<f32>(source)), vec2<i32>(0), source - vec2<i32>(1));
    let bed = textureLoad(refraction_source, texel, 0).rgb;

    // Light through water fades with the depth it crosses, red first: the
    // shallows show the bed tinted toward `WaterColor`, deep water is all
    // body. A clearer place (`WaterTransparency` up) lets light go further.
    let clarity = max(1.0 - alpha, 0.02) * WATER_CLEAR_STUDS;
    let hue = scatter / max(max(scatter.r, scatter.g), max(scatter.b, 1.0e-4));
    let through = exp(-water_depth(frag) / (clarity * (0.5 + 0.5 * hue)));
    // The bed and the reflected scene are already fogged in the copy and
    // fogged again here with the rest; far enough out for that to show,
    // the fog has swallowed them anyway.
    let color = (bed * through + body * (1.0 - through)) * (1.0 - fresnel) + mirror * fresnel + glint;
    return vec4<f32>(fade(color, to_eye, distance), 1.0);
}
