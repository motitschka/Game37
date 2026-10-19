//! Command-line options.

use std::path::PathBuf;

use crate::ui::input::{Action, Input, Pointer};

pub const USAGE: &str =
    "usage: game37 [--config <path>] [--data-dir <path>] [--screenshot <out.bmp> [--actions <a,b,...>]]";

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
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--config" => options.config = Some(PathBuf::from(value()?)),
            "--data-dir" => options.data_dir = Some(PathBuf::from(value()?)),
            "--screenshot" => out = Some(PathBuf::from(value()?)),
            "--actions" => actions = Some(value()?),
            _ => return Err(format!("unknown argument {arg:?}")),
        }
    }
    options.screenshot = match (out, actions) {
        (Some(out), actions) => Some(Screenshot {
            out,
            steps: parse_steps(actions.as_deref().unwrap_or(""))?,
        }),
        (None, Some(_)) => return Err("--actions only works with --screenshot".to_owned()),
        (None, None) => None,
    };
    Ok(options)
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
}
