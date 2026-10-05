//! The Terrain Editor panel's live controls: the sliders and number fields
//! that need entities of their own. Built the first time the panel draws
//! (an `InputState` needs a window), and brought back in line with the
//! settings every draw, since the viewport moves the same values (a region
//! drag, the `B` wheel).

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::slider::{SliderEvent, SliderState, SliderValue};
use gpui_kit::*;

use crate::terrain::Settings;

use super::super::Shell;

/// Which number a field edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum Number {
    Position(usize),
    Size(usize),
    Rotation(usize),
    PlaneY,
    FlattenY,
    Seed,
}

impl Number {
    pub(in crate::shell) const ALL: [Number; 12] = [
        Number::Position(0),
        Number::Position(1),
        Number::Position(2),
        Number::Size(0),
        Number::Size(1),
        Number::Size(2),
        Number::Rotation(0),
        Number::Rotation(1),
        Number::Rotation(2),
        Number::PlaneY,
        Number::FlattenY,
        Number::Seed,
    ];

    fn read(self, editor: &super::TerrainEditor) -> f32 {
        let settings = &editor.settings;
        match self {
            Number::Position(axis) => settings.region.center()[axis],
            Number::Size(axis) => settings.region.size()[axis],
            Number::Rotation(axis) => {
                let (x, y, z) = editor.rotation_euler();
                [x, y, z][axis]
            }
            Number::PlaneY => settings.plane_origin[1],
            Number::FlattenY => settings.flatten_y,
            Number::Seed => settings.generate.seed as f32,
        }
    }
}

/// Which slider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum Rail {
    Size,
    Height,
    Strength,
    Blending,
    BiomeSize,
}

impl Rail {
    const ALL: [Rail; 5] = [
        Rail::Size,
        Rail::Height,
        Rail::Strength,
        Rail::Blending,
        Rail::BiomeSize,
    ];

    /// `(min, max, step)`.
    fn range(self) -> (f32, f32, f32) {
        match self {
            Rail::Size | Rail::Height => (1.0, 64.0, 1.0),
            Rail::Strength => (0.1, 1.0, 0.05),
            Rail::Blending => (0.0, 1.0, 0.05),
            Rail::BiomeSize => (32.0, 2048.0, 16.0),
        }
    }

    pub(in crate::shell) fn read(self, settings: &Settings) -> f32 {
        match self {
            Rail::Size => settings.size,
            Rail::Height => settings.height,
            Rail::Strength => settings.strength,
            Rail::Blending => settings.generate.blending,
            Rail::BiomeSize => settings.generate.biome_size,
        }
    }

    fn write(self, settings: &mut Settings, value: f32) {
        match self {
            Rail::Size => settings.set_size(value),
            Rail::Height => settings.set_height(value),
            Rail::Strength => settings.set_strength(value),
            Rail::Blending => settings.generate.blending = value.clamp(0.0, 1.0),
            Rail::BiomeSize => settings.generate.biome_size = value.max(16.0),
        }
    }
}

pub(crate) struct TerrainFields {
    rails: Vec<(Rail, Entity<SliderState>)>,
    numbers: Vec<(Number, Entity<InputState>)>,
    _subscriptions: Vec<Subscription>,
}

impl TerrainFields {
    pub(in crate::shell) fn new(
        shell: &Shell,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) -> Self {
        let mut subscriptions = Vec::new();
        let rails: Vec<(Rail, Entity<SliderState>)> = Rail::ALL
            .into_iter()
            .map(|rail| {
                let (min, max, step) = rail.range();
                let value = rail.read(&shell.terrain.settings);
                let state = cx.new(|_| {
                    SliderState::new()
                        .min(min)
                        .max(max)
                        .step(step)
                        .default_value(value)
                });
                subscriptions.push(cx.subscribe(
                    &state,
                    move |shell, _, event: &SliderEvent, cx| {
                        if let SliderEvent::Change(SliderValue::Single(value)) = event {
                            rail.write(&mut shell.terrain.settings, *value);
                            shell.redraw_terrain_overlay(cx);
                            cx.notify();
                        }
                    },
                ));
                (rail, state)
            })
            .collect();
        let numbers = Number::ALL
            .into_iter()
            .map(|number| {
                let text = format_number(number, number.read(&shell.terrain));
                let input = cx.new(|cx| InputState::new(window, cx).default_value(text));
                subscriptions.push(cx.subscribe(
                    &input,
                    move |shell, input, event: &InputEvent, cx| {
                        if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                            let text = input.read(cx).value().to_string();
                            if let Ok(value) = text.trim().parse::<f32>() {
                                shell.set_terrain_number(number, value, cx);
                            }
                        }
                    },
                ));
                (number, input)
            })
            .collect();
        TerrainFields {
            rails,
            numbers,
            _subscriptions: subscriptions,
        }
    }

    pub(in crate::shell) fn rail(&self, rail: Rail) -> &Entity<SliderState> {
        &self
            .rails
            .iter()
            .find(|(r, _)| *r == rail)
            .expect("every rail is built")
            .1
    }

    pub(in crate::shell) fn number(&self, number: Number) -> &Entity<InputState> {
        &self
            .numbers
            .iter()
            .find(|(n, _)| *n == number)
            .expect("every number is built")
            .1
    }

    /// Puts every value back where something else moved it, leaving alone
    /// a field being typed into.
    pub(in crate::shell) fn sync(
        &self,
        editor: &super::TerrainEditor,
        window: &mut Window,
        cx: &mut App,
    ) {
        for (rail, state) in &self.rails {
            let value = rail.read(&editor.settings);
            if (state.read(cx).value().end() - value).abs() > 1e-4 {
                state.update(cx, |state, cx| state.set_value(value, window, cx));
            }
        }
        for (number, input) in &self.numbers {
            if input.read(cx).focus_handle(cx).is_focused(window) {
                continue;
            }
            let text = format_number(*number, number.read(editor));
            if input.read(cx).value().as_ref() != text {
                input.update(cx, |input, cx| input.set_value(text, window, cx));
            }
        }
    }
}

fn format_number(number: Number, value: f32) -> String {
    match number {
        Number::Seed => format!("{}", value as u32),
        _ => {
            let rounded = (value * 1000.0).round() / 1000.0;
            format!("{rounded}")
        }
    }
}
