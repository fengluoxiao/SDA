//! Parallel source-major excitation for continuous HRTF objects. All shared
//! audio buses are reduced after joining; source clocks/history stay exclusive.
use crate::{
    Engine, OBJECT_ACTIVITY_THRESHOLD, ObjectActivitySnapshot, ROOM_SPEAKER_REFERENCE_GAIN,
    RouteGains, Source, SourceKind, bus_renderer, convolution, source_extent, vbap,
};
use rayon::prelude::*;
pub type Controls = (
    [f32; vbap::MAX_BUS_COUNT],
    [f32; vbap::MAX_BUS_COUNT],
    f32,
    f32,
);

/// ADM events are inserted in presentation order. Avoid a tree lookup for
/// every sample while the next event is still in the future; retain the keyed
/// fallback for an out-of-order/stale event so legacy seek semantics remain
/// unchanged.
fn take_due<T>(events: &mut std::collections::BTreeMap<u64, T>, at: u64) -> Option<T> {
    match events.first_key_value().map(|(timestamp, _)| *timestamp) {
        Some(timestamp) if timestamp == at => events.pop_first().map(|(_, event)| event),
        Some(timestamp) if timestamp > at => None,
        None => None,
        Some(_) => events.remove(&at),
    }
}

#[derive(Clone, Copy)]
pub struct Context {
    pub layout: vbap::LayoutId,
    pub head: Option<[f32; 4]>,
    pub extent: source_extent::Settings,
    pub near_active: bool,
    pub near_field: crate::near_field::Settings,
    pub sample_rate: u32,
    pub start: u64,
    pub offset: usize,
    pub bass_crossover_hz: Option<f32>,
}
#[derive(Clone, Copy, Default)]
pub struct Frame {
    pub main: [f32; vbap::MAX_BUS_COUNT],
    pub reflections: [f32; vbap::MAX_BUS_COUNT],
    pub diffuse: [f32; vbap::MAX_BUS_COUNT],
    pub bass: f32,
}
pub struct Buffer {
    pub frames: Vec<Frame>,
    pub activity: Vec<ObjectActivitySnapshot>,
    pub underruns: u64,
    pub route_updates: u64,
    /// Accumulated (sum of squares, count), folded into the engine's
    /// bed-vs-object balance diagnostic after the worker joins.
    pub level_probe_bed: (f64, u64),
    pub level_probe_object: (f64, u64),
}
impl Buffer {
    pub fn new() -> Self {
        Self {
            frames: vec![Frame::default(); convolution::DEFAULT_PARTITION],
            activity: Vec::new(),
            underruns: 0,
            route_updates: 0,
            level_probe_bed: (0.0, 0),
            level_probe_object: (0.0, 0),
        }
    }
    fn reset(&mut self, activity: &[ObjectActivitySnapshot]) {
        self.frames.fill(Frame::default());
        self.activity.clear();
        self.activity.extend_from_slice(activity);
        self.level_probe_bed = (0.0, 0);
        self.level_probe_object = (0.0, 0);
        self.underruns = 0;
        self.route_updates = 0;
    }
    fn add_main_and_reflections_scaled(
        &mut self,
        main: f32,
        reflections: f32,
        route: &[f32; vbap::MAX_BUS_COUNT],
        levels: &[f32; vbap::MAX_BUS_COUNT],
        frame: usize,
        bus_count: usize,
    ) {
        // Both paths use the same object route. Walking the buses together
        // halves the route/level cache traffic for dense scenes while each
        // destination keeps its original source-order accumulation.
        for bus in 0..bus_count {
            // VBAP routes only drive their active simplex speakers. Avoid
            // rewriting the other bus inputs with an exact zero for every
            // object/sample; nonzero buses retain the same add order.
            if route[bus] == 0.0 {
                continue;
            }
            if main != 0.0 {
                self.frames[frame].main[bus] += main * route[bus] * levels[bus];
            }
            if reflections != 0.0 {
                self.frames[frame].reflections[bus] += reflections * route[bus] * levels[bus];
            }
        }
    }
    fn add_diffuse(&mut self, bus: usize, sample: f32, frame: usize) {
        self.frames[frame].diffuse[bus] += sample;
    }
    fn add_bass(&mut self, sample: f32, frame: usize) {
        self.frames[frame].bass += sample;
    }
}
pub fn mix(
    sources: &mut [&mut Source],
    buffers: &mut Vec<Buffer>,
    controls: &[Controls],
    ctx: Context,
    solver: &vbap::VbapSolver,
    activity: &[ObjectActivitySnapshot],
) -> usize {
    let pool = crate::direct_renderer::workers();
    let groups = crate::direct_renderer::worker_count_for_sources(sources.len())
        .min(pool.map_or(1, |pool| pool.current_num_threads()))
        .min(sources.len());
    let chunk = sources.len().div_ceil(groups);
    let used = sources.len().div_ceil(chunk);
    buffers.resize_with(used, Buffer::new);
    for buffer in buffers.iter_mut() {
        buffer.reset(activity);
    }
    let process = |(sources, buffer): (&mut [&mut Source], &mut Buffer)| {
        for source in sources {
            mix_source(source, buffer, controls, ctx, solver);
        }
    };
    if let Some(pool) = pool {
        pool.install(|| {
            sources
                .par_chunks_mut(chunk)
                .zip(buffers.par_iter_mut())
                .for_each(process)
        });
    } else {
        sources
            .chunks_mut(chunk)
            .zip(buffers.iter_mut())
            .for_each(process);
    }
    used
}
fn mix_source(
    source: &mut Source,
    buffer: &mut Buffer,
    controls: &[Controls],
    ctx: Context,
    vbap: &vbap::VbapSolver,
) {
    let perf_id = if crate::performance::enabled() {
        source
            .object_id
            .map_or_else(|| "object".into(), |id| format!("obj:{id}"))
    } else {
        String::new()
    };
    let _perf = crate::performance::span("object.routing_and_mix", &perf_id, controls.len() as u64);
    let head_pose = ctx.head;
    let bus_count = vbap.bus_count();
    for (offset, &(levels, background, mix, bass_mix)) in controls.iter().enumerate() {
        let at = ctx.start + offset as u64;
        let block_index = ctx.offset + offset;
        if source.remove_at.is_some_and(|remove_at| at >= remove_at) {
            Engine::record_fast_activity(source, at + 1, &mut buffer.activity);
            continue;
        }
        // Mute/unmute events must land even while suspended: an unmute
        // is what wakes the source back up.
        if let Some(muted) = take_due(&mut source.mute_events, at) {
            source.muted = muted;
            if !muted {
                source.suspended = false;
            }
        }
        // Metadata follows the codec clock even when a source is
        // suspended; its next audible sample must use the current state.
        if source.kind == SourceKind::Object {
            let mut changed = false;
            if let Some(event) = take_due(&mut source.spatial_events, at) {
                changed = Engine::start_source_motion(source, event);
            }
            // Preserve the authored motion resolution independently of
            // the FFT partition used by long room/headphone filters.
            let expanded = ctx.extent.enabled
                && (ctx.extent.width > 0.0
                    || source.extent[0] > 0.0
                    || source.extent[2] > 0.0
                    || source
                        .motion
                        .as_ref()
                        .is_some_and(|m| m.extent[0] > 0.0 || m.extent[2] > 0.0));
            let motion_quantum: u64 = if expanded { 512 } else { 128 };
            let motion_phase = if expanded {
                at % motion_quantum
            } else {
                block_index as u64 % motion_quantum
            };
            if source.motion.is_some() && (changed || motion_phase == 0) {
                Engine::route_motion_block(
                    source,
                    &vbap,
                    head_pose,
                    ctx.extent,
                    (motion_quantum - motion_phase) as u32,
                );
                buffer.route_updates = buffer.route_updates.saturating_add(1);
            } else if changed {
                Engine::set_source_route(
                    source,
                    RouteGains {
                        buses: if ctx.extent.enabled {
                            source_extent::route(
                                &vbap,
                                source.position,
                                head_pose,
                                source.extent,
                                source.horizontal_only,
                                &source.zone_exclusion,
                                ctx.extent,
                            )
                        } else {
                            bus_renderer::route_zoned(
                                &vbap,
                                source.position,
                                head_pose,
                                source.spread,
                                source.diffuse,
                                source.horizontal_only,
                                &source.zone_exclusion,
                            )
                        },
                        lfe: 0.0,
                    },
                    0,
                );
                buffer.route_updates = buffer.route_updates.saturating_add(1);
            }
        }
        if let Some(event) = take_due(&mut source.gain_events, at) {
            source.target_gain = event.gain;
            source.ramp_remaining = event.ramp;
            source.ramp_step = if event.ramp == 0 {
                source.gain = event.gain;
                0.0
            } else {
                (event.gain - source.gain) / event.ramp as f32
            };
            source.suspended = false;
        }
        if source.suspended {
            Engine::advance_source_envelopes(source, 1);
            if at % convolution::DEFAULT_PARTITION as u64 == 0
                && source.samples.has_future_pcm_within(at, 4800)
            {
                source.suspended = false;
            }
            Engine::record_fast_activity(source, at + 1, &mut buffer.activity);
            continue;
        }
        // Match master worklet timing: the event boundary emits the
        // current vector/scalar first, then advances its envelopes for
        // the following sample. Advancing here would make every moving
        // object start one step ahead of its scheduled codec sample.
        let raw = source.samples.take(at);
        if let Some(value) = raw {
            let probe = if source.kind == crate::SourceKind::Bed {
                &mut buffer.level_probe_bed
            } else {
                &mut buffer.level_probe_object
            };
            probe.0 += (value as f64) * (value as f64);
            probe.1 += 1;
        }
        let target = if raw.is_some() { 1.0 } else { 0.0 };
        if target != source.availability_target {
            // Streams legitimately encode whole silent passages per object.
            // A hard 0.67 ms edge after minutes of encoded silence is audible
            // as stutter, so re-entry fades track the silence length while
            // departures stay at the fast master ramp.
            let silence = at.saturating_sub(source.last_audible_at);
            let ramp = if target == 1.0 && silence > ctx.sample_rate as u64 {
                ctx.sample_rate / 100 // 10 ms de-pop on long-silence re-entry
            } else {
                32
            };
            source.availability_target = target;
            source.availability_ramp_remaining = ramp;
            source.availability_step = (target - source.availability) / ramp as f32;
        }
        if source.availability_ramp_remaining > 0 {
            source.availability += source.availability_step;
            source.availability_ramp_remaining -= 1;
            if source.availability_ramp_remaining == 0 {
                source.availability = source.availability_target;
            }
        }
        if raw.is_some() {
            source.last_audible_at = at;
        }
        // Enter suspend: a muted source with no queued future PCM has
        // nothing to render until an unmute or new PCM arrives. Its
        // whole body is skipped from the next block onward.
        if source.muted
            && !source.suspended
            && raw.is_none()
            && !source.samples.has_future_pcm_within(at, 4800)
            && source.gain_events.is_empty()
            && source.spatial_events.is_empty()
        {
            source.suspended = true;
        }
        let mut sample = raw.unwrap_or(0.0)
            * source.availability
            * source.gain
            * Engine::distance_gain(source)
            * if source.muted { 0.0 } else { 1.0 };
        if bass_mix > 1e-6 {
            if let Some(crossover_hz) = ctx.bass_crossover_hz {
                if source
                    .bass_split
                    .as_ref()
                    .is_none_or(|filter| filter.frequency != crossover_hz)
                {
                    source.bass_split = crate::cinema::BassSplit::new(crossover_hz).ok();
                }
                if let Some(filter) = &mut source.bass_split {
                    let (low, high) = filter.process(sample);
                    let contribution: f32 = source
                        .bus_gains
                        .iter()
                        .zip(&levels)
                        .map(|(gain, level)| gain * level)
                        .sum();
                    buffer.add_bass(low * contribution * bass_mix, block_index);
                    sample += (high - sample) * bass_mix;
                }
            }
        }
        if source.object_id.is_some() && sample.abs() >= OBJECT_ACTIVITY_THRESHOLD {
            source.activity_until =
                at.saturating_add((ctx.sample_rate as f32 * 0.2).round() as u64);
        }
        if raw.is_none() && source.gain != 0.0 {
            buffer.underruns += 1;
        }
        if source.kind == SourceKind::Object {
            let target = if ctx.extent.enabled && !source.continuous_active {
                source.diffuse.max(ctx.extent.diffusion)
            } else {
                0.0
            };
            source.diffusion_mix +=
                (target - source.diffusion_mix).clamp(-1.0 / 9600.0, 1.0 / 9600.0);
            if source.diffusion_mix > 0.0 {
                if block_index == 0 {
                    source.diffuse_route = bus_renderer::route_zoned(
                        &vbap,
                        source.position,
                        head_pose,
                        0.0,
                        1.0,
                        source.horizontal_only,
                        &source.zone_exclusion,
                    );
                }
                for bus in 0..vbap.bus_count() {
                    buffer.add_diffuse(
                        bus,
                        sample
                            * source.diffusion_mix.sqrt()
                            * source.diffuse_route[bus]
                            * levels[bus],
                        block_index,
                    );
                }
                sample *= (1.0 - source.diffusion_mix).sqrt();
            }
        }
        // ADM masters carry silent PCM for inactive objects. Keep their
        // clocks, filters and envelopes running, but avoid zero bus work.
        let bus_sample = sample * ROOM_SPEAKER_REFERENCE_GAIN * (1.0 - mix);
        if block_index == 0 {
            let mut position = source.position;
            if source.horizontal_only {
                position[2] = 0.0;
            }
            let direction = crate::directional::Direction {
                position,
                head: head_pose,
                diffuse: source.diffuse.max(if ctx.extent.enabled {
                    ctx.extent.diffusion
                } else {
                    0.0
                }),
                horizontal_only: source.horizontal_only,
                // Square the spread/extent the same way as the general path
                // (Engine::route): the continuous footprint renders the authored
                // width literally, which unstabilises vocals authored against the
                // VBAP snap path where spread only tilts bus gains.
                width: if ctx.extent.enabled {
                    let w = source.extent[0].max(ctx.extent.width);
                    w * w * 120.0
                } else {
                    source.spread * source.spread * 120.0
                },
                height: if source.horizontal_only {
                    0.0
                } else if ctx.extent.enabled {
                    let h = source.extent[2];
                    h * h * 120.0
                } else {
                    source.spread * source.spread * 120.0
                },
                depth: if ctx.extent.enabled {
                    source.extent[1]
                } else {
                    0.0
                },
            };
            source.continuous.as_mut().unwrap().schedule(
                direction,
                ctx.layout,
                std::array::from_fn(|bus| source.bus_gains[bus] * levels[bus]),
                background,
            );
        }
        if block_index % 128 == 0 {
            let near_settings = crate::near_field::Settings {
                enabled: ctx.near_active || source.distance_m.is_some(),
                ..ctx.near_field
            };
            source.near_target = if ctx.near_active {
                crate::near_field::gains(source.position, head_pose, near_settings)
            } else if let Some(position) = Engine::physical_near_position(source, near_settings) {
                crate::near_field::gains(position, head_pose, near_settings)
            } else {
                [1.0; 2]
            };
        }
        let input = sample * ROOM_SPEAKER_REFERENCE_GAIN * mix;
        let continuous = source.continuous.as_mut().unwrap();
        if block_index == 0 {
            continuous.occlusion_targets = source.occlusion_targets;
        }
        continuous.frames[block_index].input = input;
        continuous.frames[block_index].near = source.near_target;
        let reflection_sample = if input != 0.0 {
            // Near sources sit outside the reverberant field: fade their room
            // contribution as they close in (mirror of the general path).
            let norm = if ctx.near_active {
                source
                    .position
                    .iter()
                    .map(|axis| axis * axis)
                    .sum::<f32>()
                    .sqrt()
            } else {
                source
                    .distance_m
                    .map(|distance| distance / ctx.near_field.metres_per_unit)
                    .unwrap_or(1.0)
            };
            let proximity_dry = if norm < 1.0 {
                (0.25 + 0.75 * norm).max(0.25)
            } else {
                1.0
            };
            input * proximity_dry
        } else {
            0.0
        };
        if bus_sample != 0.0 || reflection_sample != 0.0 {
            buffer.add_main_and_reflections_scaled(
                bus_sample,
                reflection_sample,
                &source.bus_gains,
                &levels,
                block_index,
                bus_count,
            );
        }
        Engine::advance_source_envelopes(source, 1);
        Engine::record_fast_activity(source, at + 1, &mut buffer.activity);
    }
}
