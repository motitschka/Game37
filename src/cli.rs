//! Command-line options.

use std::path::PathBuf;

use crate::emulators::{self, Emulators};
use crate::systems::{self, Kind};
use crate::ui::input::{Action, Input, Pointer, TextInput};

pub const USAGE: &str = "usage: game37 [--config <path>] [--data-dir <path>] \
[--screenshot <out.bmp> [--actions <a,b,...>] [--missing-emulators <system ids>] [--frames <n>] \
[--artwork] [--screen controller-test]]";

#[derive(Debug, Default, PartialEq)]
pub struct Options {
    /// Overrides `config::config_path()`.
    pub config: Option<PathBuf>,
    /// Overrides `config::data_dir()`, where the library database lives.
    pub data_dir: Option<PathBuf>,
    pub screenshot: Option<Screenshot>,
}

/// Headless run: apply `steps`, one per frame, draw `frames` more frames, then write the last as a
/// BMP and exit.
#[derive(Debug, PartialEq)]
pub struct Screenshot {
    pub out: PathBuf,
    pub steps: Vec<Step>,
    /// Which Systems' Standalone Emulators count as missing; every other emulator counts as
    /// installed (FR-CAR-9). `Emulators::all_installed()` when `--missing-emulators` wasn't given.
    pub missing_emulators: Emulators,
    /// Frames drawn after the steps, at least 1 (`--frames`): a running Game advances one frame
    /// per step and per frame here.
    pub frames: u32,
    /// `--artwork`: show Artwork as the app does, cached under the data dir and fetched with the
    /// key in `secrets.toml` next to the Config, waiting for it before each frame. Without it every
    /// Game shows placeholder art and nothing is read or fetched.
    pub artwork: bool,
    /// `--screen`: a screen to open over the start screen before the steps, with the state it shows
    /// faked.
    pub screen: Option<ShotScreen>,
}

/// A screen `--screen` opens, for screens no step sequence can reach on its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShotScreen {
    /// `controller-test`: the Controller test over Settings, with fake controllers connected.
    ControllerTest,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Action(Action),
    /// A left click at logical coordinates.
    Click(f32, f32),
    /// `wait:N`: N frames without input, e.g. to let a running Game get going.
    Wait(u32),
    /// `text:CHARS`: typing CHARS on the keyboard into the active text field, all in one frame.
    Text(String),
}

impl Step {
    /// How many frames the step takes.
    pub fn frames(&self) -> u32 {
        if let Step::Wait(n) = *self { n } else { 1 }
    }

    /// The inputs the step stands for: a click is preceded by the pointer moving there.
    pub fn inputs(&self) -> Vec<Input> {
        match *self {
            Step::Action(a) => vec![Input::Action(a)],
            Step::Wait(_) => Vec::new(),
            Step::Text(ref text) => text.chars().map(|ch| Input::Text(TextInput::Char(ch))).collect(),
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
    let mut frames = None;
    let mut artwork = false;
    let mut screen = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--config" => options.config = Some(PathBuf::from(value()?)),
            "--data-dir" => options.data_dir = Some(PathBuf::from(value()?)),
            "--screenshot" => out = Some(PathBuf::from(value()?)),
            "--actions" => actions = Some(value()?),
            "--missing-emulators" => missing_emulators = Some(value()?),
            "--frames" => frames = Some(value()?),
            "--artwork" => artwork = true,
            "--screen" => screen = Some(value()?),
            _ => return Err(format!("unknown argument {arg:?}")),
        }
    }
    let Some(out) = out else {
        let given = [
            ("--actions", actions.is_some()),
            ("--missing-emulators", missing_emulators.is_some()),
            ("--frames", frames.is_some()),
            ("--artwork", artwork),
            ("--screen", screen.is_some()),
        ];
        if let Some((flag, _)) = given.iter().find(|(_, given)| *given) {
            return Err(format!("{flag} only works with --screenshot"));
        }
        return Ok(options);
    };
    if options.config.is_none() || options.data_dir.is_none() {
        return Err(
            "--screenshot needs --config and --data-dir, so it never touches your real Config and data"
                .to_owned(),
        );
    }
    options.screenshot = Some(Screenshot {
        out,
        steps: parse_steps(actions.as_deref().unwrap_or(""))?,
        missing_emulators: parse_missing_emulators(missing_emulators.as_deref())?,
        frames: match frames {
            None => 1,
            Some(n) => n
                .parse()
                .ok()
                .filter(|&n| n >= 1)
                .ok_or_else(|| format!("bad --frames {n:?}: expected a whole number from 1"))?,
        },
        artwork,
        screen: match screen.as_deref() {
            None => None,
            Some("controller-test") => Some(ShotScreen::ControllerTest),
            Some(other) => return Err(format!("bad --screen {other:?}: expected controller-test")),
        },
    });
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
    if let Some(n) = step.strip_prefix("wait:") {
        return n
            .parse()
            .map(Step::Wait)
            .map_err(|_| format!("bad wait {step:?}: expected wait:N frames"));
    }
    if let Some(rest) = step.strip_prefix("click:") {
        let coords = rest
            .split_once(':')
            .and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)));
        return coords.map(|(x, y)| Step::Click(x, y)).ok_or_else(|| {
            format!("bad click {step:?}: expected click:X:Y in logical 1920x1080 coordinates")
        });
    }
    if let Some(text) = step.strip_prefix("text:") {
        if text.is_empty() {
            return Err(format!("bad text {step:?}: expected text:CHARS to type"));
        }
        return Ok(Step::Text(text.to_owned()));
    }
    Action::from_name(step).map(Step::Action).ok_or_else(|| {
        let names: Vec<&str> = Action::ALL.iter().map(|a| a.name()).collect();
        format!(
            "unknown action {step:?}: expected one of {}, click:X:Y or text:CHARS",
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

    /// `args`, with `--config` and `--data-dir` always supplied, for tests whose `--screenshot`
    /// isn't itself about that requirement.
    fn shot_args(s: &str) -> Vec<String> {
        shot_args_vec(args(s))
    }

    fn shot_args_vec(extra: Vec<String>) -> Vec<String> {
        let mut out = args("--config c.toml --data-dir d");
        out.extend(extra);
        out
    }

    #[test]
    fn screenshot_needs_config_and_data_dir() {
        let err = parse(args("--screenshot out.bmp")).unwrap_err();
        assert!(err.contains("--config") && err.contains("--data-dir"), "{err}");
        let err = parse(args("--screenshot out.bmp --config c.toml")).unwrap_err();
        assert!(err.contains("--data-dir"), "{err}");
        let err = parse(args("--screenshot out.bmp --data-dir d")).unwrap_err();
        assert!(err.contains("--config"), "{err}");
        assert!(parse(shot_args("--screenshot out.bmp")).is_ok());
    }

    #[test]
    fn screenshot_with_actions() {
        let options = parse(shot_args(
            "--screenshot out.bmp --actions nexttab,click:900:60,wait:30,back",
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
                Step::Wait(30),
                Step::Action(Action::Back)
            ]
        );
    }

    #[test]
    fn frames_default_to_one_and_take_a_count() {
        assert_eq!(
            parse(shot_args("--screenshot o.bmp"))
                .unwrap()
                .screenshot
                .unwrap()
                .frames,
            1
        );
        let shot = parse(shot_args("--screenshot o.bmp --actions confirm --frames 180"))
            .unwrap()
            .screenshot
            .unwrap();
        assert_eq!((shot.frames, shot.steps.len()), (180, 1));
        for bad in ["0", "-1", "ten", ""] {
            let err = parse(shot_args_vec(vec![
                "--screenshot".into(),
                "o.bmp".into(),
                "--frames".into(),
                bad.into(),
            ]))
            .unwrap_err();
            assert!(err.contains("--frames"), "{err}");
        }
        let err = parse(args("--frames 2")).unwrap_err();
        assert!(err.contains("--screenshot"), "{err}");
        assert!(USAGE.contains("[--frames <n>]"));
    }

    #[test]
    fn artwork_is_off_unless_asked_for_and_only_works_with_screenshot() {
        let shot = |a: &str| parse(shot_args(a)).unwrap().screenshot.unwrap();
        assert!(!shot("--screenshot o.bmp").artwork);
        assert!(shot("--screenshot o.bmp --artwork --frames 2").artwork);
        assert!(shot("--artwork --screenshot o.bmp").artwork);
        let err = parse(args("--artwork")).unwrap_err();
        assert!(err.contains("--artwork") && err.contains("--screenshot"), "{err}");
        assert!(USAGE.contains("[--artwork]"));
    }

    #[test]
    fn screen_opens_the_controller_test_and_only_works_with_screenshot() {
        let shot = |a: &str| parse(shot_args(a)).unwrap().screenshot.unwrap();
        assert_eq!(shot("--screenshot o.bmp").screen, None);
        assert_eq!(
            shot("--screen controller-test --screenshot o.bmp").screen,
            Some(ShotScreen::ControllerTest)
        );
        let err = parse(args("--screen controller-test")).unwrap_err();
        assert!(err.contains("--screen") && err.contains("--screenshot"), "{err}");
        let err = parse(shot_args("--screenshot o.bmp --screen settings")).unwrap_err();
        assert!(err.contains("controller-test"), "{err}");
        assert!(USAGE.contains("[--screen controller-test]"));
    }

    #[test]
    fn no_arguments_run_the_app() {
        assert_eq!(parse(args("")).unwrap(), Options::default());
    }

    #[test]
    fn unknown_action_is_an_error() {
        let err = parse(shot_args("--screenshot o.bmp --actions jump")).unwrap_err();
        assert!(err.contains("jump"), "{err}");
        assert!(parse(shot_args("--screenshot o.bmp --actions click:1")).is_err());
        assert!(parse(shot_args("--screenshot o.bmp --actions text:")).is_err());
    }

    #[test]
    fn a_text_step_types_each_character_in_one_frame() {
        let shot = parse(shot_args("--screenshot o.bmp --actions search,text:Mé1,down"))
            .unwrap()
            .screenshot
            .unwrap();
        assert_eq!(
            shot.steps,
            [
                Step::Action(Action::Search),
                Step::Text("Mé1".into()),
                Step::Action(Action::Down)
            ]
        );
        let typed = |ch| Input::Text(TextInput::Char(ch));
        assert_eq!(shot.steps[1].inputs(), [typed('M'), typed('é'), typed('1')]);
        assert_eq!(shot.steps[1].frames(), 1);
    }

    #[test]
    fn misuse_is_an_error() {
        assert!(parse(args("--actions up")).is_err());
        assert!(parse(args("--config")).is_err());
        assert!(parse(args("--fast")).is_err());
    }

    #[test]
    fn missing_emulators_marks_only_the_named_systems() {
        let options = parse(shot_args("--screenshot o.bmp --missing-emulators wiiu,ps3")).unwrap();
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
        let options = parse(shot_args("--screenshot o.bmp")).unwrap();
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
        let err = parse(shot_args("--screenshot o.bmp --missing-emulators dreamcast")).unwrap_err();
        assert!(err.contains("dreamcast"), "{err}");
    }

    #[test]
    fn an_embedded_system_id_is_an_error() {
        let err = parse(shot_args("--screenshot o.bmp --missing-emulators snes")).unwrap_err();
        assert!(err.contains("snes"), "{err}");
    }
}
