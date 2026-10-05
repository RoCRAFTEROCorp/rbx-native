
// `Terrain.Decoration`'s animated grass, concatenated after `atmosphere.wgsl`
// (bind group 0, `plastic`, `shade`).
//
// One small tapered blade mesh, instanced once per blade: the instance
// carries where it stands and four random bytes, and everything else (its
// height, its lean in the wind, its fade with distance) is worked out here.
// Roblox publishes nothing about its grass beyond `GrassLength` and
// `GlobalWind`; the shape and the sway are matched by eye.

struct GrassLook {
    // Linear root colour in xyz, the longest blade in studs in w.
    root: vec4<f32>,
    // Linear tip colour in xyz, the quality level's density (0 to 1) in w.
    tip: vec4<f32>,
    // The wind's unit direction in xz, how far it leans the blades in w.
    wind: vec4<f32>,
    // x: the reach in studs, y: whole flutter cycles per shimmer period,
    // z: whole gust cycles per period. Whole, so the loop has no seam.
    params: vec4<f32>,
}

@group(1) @binding(0) var<uniform> grass: GrassLook;

struct BladeVertex {
    // x: the side, -1 to 1 (0 at the tip); y: the height along the blade, 0 to 1.
    @location(0) corner: vec2<f32>,
}

struct BladeInstance {
    @location(1) root: vec3<f32>,
    // Yaw, height variation, sway offset, rank; each 0 to 1.
    @location(2) shape: vec4<f32>,
}

struct GrassOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    // Horizontal, perpendicular to the blade; the fragment turns it to the eye.
    @location(1) face: vec3<f32>,
    @location(2) color: vec3<f32>,
}

const GRASS_TAU: f32 = 6.2831853;
// Root width in studs: broad enough to read as a blade from a few studs off.
const GRASS_WIDTH: f32 = 0.35;

// `scatter::density_at`, per blade.
fn grass_density(studs: f32) -> f32 {
    let reach = grass.params.x;
    if reach <= 0.0 {
        return 0.0;
    }
    return grass.tip.w * clamp((reach - studs) / (reach * (2.0 / 3.0)), 0.0, 1.0);
}

@vertex
fn vs_main(vertex: BladeVertex, blade: BladeInstance) -> GrassOut {
    let distance = length(lighting.camera.xyz - blade.root);
    // A blade shrinks away just before its rank drops out of the density
    // wanted where it stands, so thinning with distance never pops.
    let grow = saturate((grass_density(distance) - blade.shape.w) * 12.0);
    let height = grass.root.w * mix(0.55, 1.2, blade.shape.y) * grow;
    let t = vertex.corner.y;

    let yaw = blade.shape.x * GRASS_TAU;
    let across = vec3<f32>(cos(yaw), 0.0, sin(yaw));
    let wind = vec3<f32>(grass.wind.x, 0.0, grass.wind.z);
    let phase = uniforms.viewport.z;
    let downwind = dot(blade.root.xz, wind.xz);
    // A slow gust rolling across the field and a quicker flutter of each
    // blade, both travelling downwind.
    let gust = 0.5 + 0.5 * sin(GRASS_TAU * (grass.params.z * phase - downwind / 70.0));
    let flutter = sin(GRASS_TAU * (grass.params.y * phase - downwind / 15.0 + blade.shape.z));
    let lean = grass.wind.w * (0.35 + 0.65 * gust) + 0.08 * flutter;
    // Each blade also rests leaning its own way, which breaks up the rows.
    let rest_angle = fract(blade.shape.z * 7.31) * GRASS_TAU;
    let rest = vec3<f32>(cos(rest_angle), 0.0, sin(rest_angle)) * 0.25;
    let bend = wind * lean + rest;
    // Curved over, not sheared: the tip sweeps along an arc, keeping roughly
    // the blade's length.
    let offset = bend * height * t * t;
    let rise = height * t / sqrt(1.0 + dot(bend, bend) * t * t);

    let width = GRASS_WIDTH * mix(0.75, 1.25, blade.shape.y) * pow(1.0 - t, 0.7);
    let world = blade.root + across * (vertex.corner.x * width * 0.5) + offset
        + vec3<f32>(0.0, rise, 0.0);

    var out: GrassOut;
    out.clip_position = uniforms.view_projection * vec4<f32>(world, 1.0);
    out.world_position = world;
    out.face = vec3<f32>(-across.z, 0.0, across.x);
    let tone = mix(0.85, 1.1, fract(blade.shape.z * 3.17));
    out.color = mix(grass.root.rgb, grass.tip.rgb, t) * tone;
    return out;
}

@fragment
fn fs_main(in: GrassOut, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    // Mostly the ground's own up, so a field reads as lit like the slope it
    // stands on; a little of the blade's face, turned to the eye, for relief.
    let face = select(-in.face, in.face, front);
    let normal = normalize(vec3<f32>(0.0, 1.0, 0.0) + face * 0.5);
    var surface = plastic(in.color, normal, in.world_position, 0.0);
    surface.spec_strength = 0.0;
    return vec4<f32>(shade(surface), 1.0);
}
