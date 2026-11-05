use super::{InputPurpose, Menu, RouteKey};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scenario {
    #[default]
    MenuInputs,
    ShootingMenu,
    LiveView,
    FilmSimulation,
    EditorCancel,
    EditorConfirm,
    RepeatedInputs,
}

impl std::str::FromStr for Scenario {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "menu-inputs" => Ok(Self::MenuInputs),
            "shooting-menu" => Ok(Self::ShootingMenu),
            "live-view" => Ok(Self::LiveView),
            "film-simulation" => Ok(Self::FilmSimulation),
            "editor-cancel" => Ok(Self::EditorCancel),
            "editor-confirm" => Ok(Self::EditorConfirm),
            "repeated-inputs" => Ok(Self::RepeatedInputs),
            _ => Err(format!("unknown composed-startup scenario: {value}")),
        }
    }
}

#[derive(Clone, Copy)]
pub struct ScenarioInput {
    pub key: RouteKey,
    pub purpose: InputPurpose,
    pub menu: Option<Menu>,
    pub live_view: bool,
}

const fn step(key: RouteKey, purpose: InputPurpose, menu: u32, setting: u32, cursor: u32) -> ScenarioInput {
    ScenarioInput { key, purpose, menu: Some(Menu { menu, setting, cursor }), live_view: false }
}

impl Scenario {
    pub const FIRST_SETTLED: usize = 15;

    pub fn inputs(self) -> impl Iterator<Item = (RouteKey, InputPurpose)> {
        std::iter::repeat_n((RouteKey::Up, InputPurpose::FirstStart), 12)
            .chain(std::iter::repeat_n((RouteKey::Ok, InputPurpose::FirstStart), 3))
            .chain(self.settled().iter().map(|input| (input.key, input.purpose)))
    }

    pub fn settled(self) -> &'static [ScenarioInput] {
        use InputPurpose::*;
        use RouteKey::*;
        // E-MODAL-MENU-OKPATHS-150, crawl-5: nodes 1, 55 and 74..79; E-MODAL-MENU-TABLES-150: 0x78ea98 and 0x7823a4.
        const FILM: &[ScenarioInput] = &[
            step(Down, Ordinary, 1, 5, 3), step(Down, Ordinary, 1, 5, 4),
            step(Down, Ordinary, 1, 5, 5), step(Down, Ordinary, 1, 5, 6),
            step(Ok, FilmOpen, 1, 254, 0), step(Back, BackHandback, 1, 5, 6), step(Ok, FilmReentry, 1, 254, 0),
        ];
        // E-MODAL-MENU-OKPATHS-150, crawl-5: 1 -> 156 -> 225 -> 226 -> 227 -> 228; OK/BACK from 228 -> 227.
        const CANCEL: &[ScenarioInput] = &[
            step(Up, Ordinary, 1, 5, 32), step(Left, Ordinary, 0, 5, 4), step(Down, Ordinary, 0, 5, 8),
            step(Ok, Ordinary, 63, 5, 0), step(Ok, EditorOpen, 67, 56, 0), step(Back, EditorCancel, 63, 5, 0),
        ];
        const CONFIRM: &[ScenarioInput] = &[
            step(Up, Ordinary, 1, 5, 32), step(Left, Ordinary, 0, 5, 4), step(Down, Ordinary, 0, 5, 8),
            step(Ok, Ordinary, 63, 5, 0), step(Ok, EditorOpen, 67, 56, 0), step(Ok, EditorConfirm, 63, 5, 0),
        ];
        const LIVE: ScenarioInput = ScenarioInput { key: Back, purpose: LiveView, menu: None, live_view: true };
        const MENU_INPUTS: &[ScenarioInput] = &[
            step(Down, Ordinary, 1, 5, 3), step(Up, Ordinary, 1, 5, 2),
            ScenarioInput { purpose: BackHandback, ..LIVE },
            ScenarioInput { key: Ok, purpose: AfterBack, menu: None, live_view: false },
            ScenarioInput { key: Left, purpose: UnhandledProbe, menu: None, live_view: false },
        ];
        const SHOOTING: &[ScenarioInput] = &[step(Down, Ordinary, 1, 5, 3), step(Up, Ordinary, 1, 5, 2)];
        const LIVE_VIEW: &[ScenarioInput] = &[LIVE];
        const REPEATED: &[ScenarioInput] = &[
            step(Down, Ordinary, 1, 5, 3), step(Down, Ordinary, 1, 5, 4), step(Down, Ordinary, 1, 5, 5),
            step(Up, Ordinary, 1, 5, 4), step(Up, Ordinary, 1, 5, 3), step(Up, Ordinary, 1, 5, 2),
        ];
        match self {
            Self::MenuInputs => MENU_INPUTS,
            Self::ShootingMenu => SHOOTING,
            Self::LiveView => LIVE_VIEW,
            Self::FilmSimulation => FILM,
            Self::EditorCancel => CANCEL,
            Self::EditorConfirm => CONFIRM,
            Self::RepeatedInputs => REPEATED,
        }
    }
}
