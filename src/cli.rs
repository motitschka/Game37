//! Command-line options.

use std::path::PathBuf;

use crate::emulators::{self, Emulators};
use crate::systems::{self, Kind};
use crate::ui::input::{Action, Input, Pointer};

pub const USAGE: &str = "usage: game37 [--config <path>] [--data-dir <path>] \
[--screenshot <out.bmp> [--actions <a,b,...>] [--missing-emulators <system ids>]]";

#[derive(Debug, Default, PartialEq)]
pub struct Options {
    /// Overrides `config::config_path()`.
    pub config: Option<PathBuf>,
    /// Overrides `config::data_dir()`, where the library database lives.
    pub data_dir: Option<PathBuf>,
    pub screenshot: Option<Screenshot>,
}

/// Headless run: apply `steps`, one per frame, then write the frame as a BMP and exit.
#[derive(Debug, PartialEq)]
pub struct Screenshot {
    pub out: PathBuf,
    pub steps: Vec<Step>,
    /// Which Systems' Standalone Emulators count as missing; every other emulator counts as
    /// installed (FR-CAR-9). `Emulators::all_installed()` when `--missing-emulators` wasn't given.
    pub missing_emulators: Emulators,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    Action(Action),
    /// A left click at logical coordinates.
    Click(f32, f32),
}

impl Step {
    /// The inputs the step stands for: a click is preceded by the pointer moving there.
    pub fn inputs(self) -> Vec<Input> {
        match self {
            Step::Action(a) => vec![Input::Action(a)],
            Step::Click(x, y) => {
                vec![
                    Input::Pointer(Pointer::Move { x, y }),
                    Input::Pointer(Pointer::Click { x, y }),
                ]
            }
        }
    }
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut options = Options::default();
    let mut out = None;
    let mut actions = None;
    let mut missing_emulators = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--config" => options.config = Some(PathBuf::from(value()?)),
            "--data-dir" => options.data_dir = Some(PathBuf::from(value()?)),
            "--screenshot" => out = Some(PathBuf::from(value()?)),
            "--actions" => actions = Some(value()?),
            "--missing-emulators" => missing_emulators = Some(value()?),
            _ => return Err(format!("unknown argument {arg:?}")),
        }
    }
    options.screenshot = match (out, actions, missing_emulators) {
        (Some(out), actions, missing_emulators) => Some(Screenshot {
            out,
            steps: parse_steps(actions.as_deref().unwrap_or(""))?,
            missing_emulators: parse_missing_emulators(missing_emulators.as_deref())?,
        }),
        (None, Some(_), _) => return Err("--actions only works with --screenshot".to_owned()),
        (None, None, Some(_)) => {
            return Err("--missing-emulators only works with --screenshot".to_owned());
        }
        (None, None, None) => None,
    };
    Ok(options)
}

/// The `--missing-emulators` ids, resolved to the `Emulators` state they stand for: every
/// emulator installed except those Systems'. An unknown or Embedded System id is an error.
fn parse_missing_emulators(list: Option<&str>) -> Result<Emulators, String> {
    let Some(list) = list else {
        return Ok(Emulators::all_installed());
    };
    let mut missing = Vec::new();
    for id in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let system = systems::by_id(id).ok_or_else(|| format!("unknown System {id:?}"))?;
        if system.kind != Kind::External {
            return Err(format!(
                "{id:?} is an Embedded System: it has no Standalone Emulator"
            ));
        }
        missing.push(system);
    }
    let missing_ids: Vec<&str> = missing
        .iter()
        .filter_map(|s| emulators::for_system(s))
        .map(|e| e.flatpak_id)
        .collect();
    let installed = systems::SYSTEMS
        .iter()
        .filter_map(emulators::for_system)
        .filter(|e| !missing_ids.contains(&e.flatpak_id))
        .map(|e| e.flatpak_id.to_owned());
    Ok(Emulators::from_installed(installed))
}

fn parse_steps(list: &str) -> Result<Vec<Step>, String> {
    list.split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| parse_step(s.trim()))
        .collect()
}

fn parse_step(step: &str) -> Result<Step, String> {
    if let Some(rest) = step.strip_prefix("click:") {
        let coords = rest
            .split_once(':')
            .and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)));
        return coords.map(|(x, y)| Step::Click(x, y)).ok_or_else(|| {
            format!("bad click {step:?}: expected click:X:Y in logical 1920x1080 coordinates")
        });
    }
    Action::from_name(step).map(Step::Action).ok_or_else(|| {
        let names: Vec<&str> = Action::ALL.iter().map(|a| a.name()).collect();
        format!(
            "unknown action {step:?}: expected one of {}, or click:X:Y",
            names.join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn screenshot_with_actions() {
        let options = parse(args(
            "--screenshot out.bmp --config c.toml --actions nexttab,click:900:60,back",
        ))
        .unwrap();
        assert_eq!(options.config, Some(PathBuf::from("c.toml")));
        let shot = options.screenshot.unwrap();
        assert_eq!(shot.out, PathBuf::from("out.bmp"));
        assert_eq!(
            shot.steps,
            [
                Step::Action(Action::NextTab),
                Step::Click(900.0, 60.0),
                Step::Action(Action::Back)
            ]
        );
    }

    #[test]
    fn no_arguments_run_the_app() {
        assert_eq!(parse(args("")).unwrap(), Options::default());
    }

    #[test]
    fn unknown_action_is_an_error() {
        let err = parse(args("--screenshot o.bmp --actions jump")).unwrap_err();
        assert!(err.contains("jump"), "{err}");
        assert!(parse(args("--screenshot o.bmp --actions click:1")).is_err());
    }

    #[test]
    fn misuse_is_an_error() {
        assert!(parse(args("--actions up")).is_err());
        assert!(parse(args("--config")).is_err());
        assert!(parse(args("--fast")).is_err());
    }

    #[test]
    fn missing_emulators_marks_only_the_named_systems() {
        let options = parse(args("--screenshot o.bmp --missing-emulators wiiu,ps3")).unwrap();
        let emulators = options.screenshot.unwrap().missing_emulators;
        assert_eq!(
            emulators.missing(systems::by_id("wiiu").unwrap()),
            Some(&emulators::CEMU)
        );
        assert_eq!(
            emulators.missing(systems::by_id("ps3").unwrap()),
            Some(&emulators::RPCS3)
        );
        assert_eq!(
            emulators.missing(systems::by_id("gc").unwrap()),
            None,
            "Dolphin isn't named"
        );
    }

    #[test]
    fn no_missing_emulators_flag_leaves_everything_installed() {
        let options = parse(args("--screenshot o.bmp")).unwrap();
        let emulators = options.screenshot.unwrap().missing_emulators;
        assert_eq!(emulators.missing(systems::by_id("wiiu").unwrap()), None);
    }

    #[test]
    fn missing_emulators_without_screenshot_is_an_error() {
        let err = parse(args("--missing-emulators wiiu")).unwrap_err();
        assert!(err.contains("--screenshot"), "{err}");
    }

    #[test]
    fn an_unknown_system_id_is_an_error() {
        let err = parse(args("--screenshot o.bmp --missing-emulators dreamcast")).unwrap_err();
        assert!(err.contains("dreamcast"), "{err}");
    }

    #[test]
    fn an_embedded_system_id_is_an_error() {
        let err = parse(args("--screenshot o.bmp --missing-emulators snes")).unwrap_err();
        assert!(err.contains("snes"), "{err}");
    }
}
