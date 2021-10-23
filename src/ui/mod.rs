use iced::container::{Style, StyleSheet};
use iced::svg::Handle;
use iced::window::Mode;
use iced::{
    button, container, executor, scrollable, text_input, time, window, Align, Application,
    Background, Button, Clipboard, Color, Column, Command, Container, Element, Font,
    HorizontalAlignment, Image, Length, Row, Rule, Scrollable, Settings, Space, Subscription, Svg,
    Text, TextInput, VerticalAlignment,
};
use notify::DebouncedEvent;
use tokio::task::spawn_blocking;
use tokio::time::Instant;
use tracing::{debug, error, info, instrument, warn};

use civfun_gmr::api::{Game, GetGamesAndPlayers, Player, UserId};
use civfun_gmr::manager::{AuthState, Config, GameInfo, Manager};
use enter_auth_key::AuthKeyScreen;
use prefs::Prefs;
use style::{button_row, cog_icon, done_icon, normal_text, steam_icon, title, ActionButtonStyle};

use crate::ui::enter_auth_key::AuthKeyMessage;
use crate::{TITLE, VERSION};

mod enter_auth_key;
mod prefs;
mod style;

pub fn run() -> anyhow::Result<()> {
    let settings = Settings {
        window: window::Settings {
            size: (400, 400),
            min_size: Some((400, 200)),
            ..Default::default()
        },
        ..Default::default()
    };
    CivFunUi::run(settings)?;
    Ok(())
}

#[derive(PartialEq, Debug, Clone)]
pub enum Screen {
    NothingYet,
    Error(String),
    AuthKeyInput,
    Games,
}

impl Screen {
    pub fn should_show_actions(&self) -> bool {
        match self {
            Screen::Games => true,
            Screen::Error(_) => true,
            _ => false,
        }
    }
}

impl Default for Screen {
    fn default() -> Self {
        Screen::NothingYet
    }
}

#[derive(Default, Debug)]
pub struct CivFunUi {
    screen: Screen,
    // settings_visible is not part of Screen so that screen can change while the settings are showing.
    settings_visible: bool,

    status_text: String,
    manager: Option<Manager>,

    actions: Actions,
    prefs: Prefs,
    enter_auth_key: AuthKeyScreen,
    games: Games,

    scroll_state: scrollable::State,
    refresh_started_at: Option<Instant>,
}

#[derive(Debug, Clone)]
pub enum Message {
    ManagerLoaded(Manager),
    AuthResponse(Option<UserId>),
    SetScreen(Screen),
    RequestRefresh(()),
    HasRefreshed(()),
    StartedWatching(()),
    ProcessTransfers,
    ProcessNewSaves,
    // AuthKeyInputChanged(String),
    // AuthKeySave,
    PlayCiv,
    SetSettingsVisibility(bool),

    AuthKeyMessage(AuthKeyMessage),
    AuthKeySave(String),
}

// TODO: Return Result<> (not anyhow::Result)
async fn fetch(mut manager: Manager) {
    manager.refresh().await.unwrap(); // TODO: unwrap
}

#[instrument(skip(manager))]
fn fetch_cmd(manager: &Option<Manager>) -> Command<Message> {
    debug!("Attempt to fetch.");
    let manager = match manager {
        Some(ref m) => m.clone(),
        None => {
            warn!("Manager not set while trying to fetch.");
            return Command::none();
        }
    };

    let is_auth_ready = manager.auth_ready();
    if !is_auth_ready {
        return Command::none();
    }

    let mut manager = manager.clone();
    Command::perform(
        async {
            fetch(manager).await;
        },
        Message::HasRefreshed,
    )
}

#[instrument(skip(manager))]
fn watch_cmd(manager: &Option<Manager>) -> Command<Message> {
    let manager = match manager {
        Some(ref m) => m.clone(),
        None => {
            warn!("Manager not set while trying to fetch.");
            return Command::none();
        }
    };

    Command::perform(
        async move { manager.start_watching_saves().await.unwrap() },
        Message::StartedWatching,
    ) // TODO: unwrap
}

async fn authenticate(mut manager: Manager) -> Option<UserId> {
    manager.authenticate().await.unwrap()
}

impl Application for CivFunUi {
    type Executor = executor::Default;
    type Message = Message;
    type Flags = ();

    fn new(_flags: ()) -> (CivFunUi, Command<Self::Message>) {
        (
            CivFunUi::default(),
            Command::perform(prepare_manager(), Message::ManagerLoaded),
        )
    }

    fn title(&self) -> String {
        format!("{} v{}", TITLE, VERSION)
    }

    #[instrument(skip(self, _clipboard, message))]
    fn update(
        &mut self,
        message: Self::Message,
        _clipboard: &mut Clipboard,
    ) -> Command<Self::Message> {
        use Message::*;
        match message {
            ManagerLoaded(mut manager) => {
                debug!("ManagerLoaded");
                self.manager = Some(manager.clone());

                if manager.auth_ready() {
                    debug!("☑ Has auth key.");
                    self.status_text = "Refreshing...".into();
                    return Command::batch([
                        fetch_cmd(&Some(manager.clone())),
                        watch_cmd(&Some(manager.clone())),
                        Command::perform(authenticate(manager.clone()), AuthResponse),
                    ]);
                } else {
                    self.screen = Screen::AuthKeyInput;
                }
            }

            AuthKeyMessage(message) => return self.enter_auth_key.update(message, _clipboard),

            AuthKeySave(auth_key) => {
                if let Some(ref mut manager) = self.manager {
                    // TODO: unwrap
                    manager.save_auth_key(&auth_key).unwrap();
                    self.status_text = "Refreshing...".into(); // TODO: make a fn for these two.
                    self.screen = Screen::Games;
                    return Command::batch([
                        Command::perform(async { Screen::Games }, Message::SetScreen),
                        Command::perform(async { () }, Message::RequestRefresh),
                    ]);
                } else {
                    error!("Manager not initialised while trying to save auth_key.");
                }
            }

            AuthResponse(Some(_)) => {
                debug!("Authenticated");
                self.screen = Screen::Games;
            }
            AuthResponse(None) => {
                debug!("Bad authentication");
                self.screen = Screen::Error("Bad authentication".into());
            }
            SetScreen(screen) => {
                self.screen = screen;
            }
            RequestRefresh(()) => {
                debug!("RequestRefresh");
                self.status_text = "Refreshing...".into();
                return fetch_cmd(&self.manager);
            }
            StartedWatching(()) => {
                debug!("StartedWatching");
            }
            HasRefreshed(()) => {
                debug!("HasRefreshed");
                self.status_text = "".into();
            }
            ProcessTransfers => {
                if let Some(ref mut manager) = self.manager {
                    let mut manager = manager.clone();
                    tokio::spawn(async move {
                        manager.process_transfers().await.unwrap();
                    });
                }
            }
            ProcessNewSaves => {
                if let Some(ref mut manager) = self.manager {
                    manager.process_new_saves().unwrap();
                }
            }
            PlayCiv => {
                // TODO: DX version from settings.
                open::that("steam://rungameid/8930//%5Cdx9").unwrap(); // TODO: unwrap
            }
            SetSettingsVisibility(v) => self.settings_visible = v,
        }
        Command::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            time::every(std::time::Duration::from_secs(60)).map(|_| Message::RequestRefresh(())),
            time::every(std::time::Duration::from_millis(1000)).map(|_| Message::ProcessTransfers),
            time::every(std::time::Duration::from_millis(1000)).map(|_| Message::ProcessNewSaves),
        ])
    }

    fn view(&mut self) -> Element<Self::Message> {
        let Self {
            manager,
            screen,
            actions,
            prefs: settings,
            settings_visible,
            scroll_state,
            enter_auth_key,
            games,
            ..
        } = self;

        let mut content = match screen {
            Screen::NothingYet => Text::new("Something funny is going on!").into(),
            Screen::AuthKeyInput => enter_auth_key.view().map(Message::AuthKeyMessage),
            Screen::Games => match &self.manager {
                Some(m) => games.view(m.games().as_slice()),
                None => Text::new("No manager yet!").into(),
            },
            Screen::Error(msg) => Text::new(format!("Error!\n\n{}", msg)).into(),
        };

        if *settings_visible {
            content = settings.view();
        }

        // TODO: Turn content to scrollable
        // let content = Scrollable::new(&mut self.scroll)
        //     .width(Length::Fill)
        //     .height(Length::Fill)
        //     .push(content());

        // let mut actions = Actions::default();

        let mut layout = Column::new();
        layout = layout.push(title());
        if !*settings_visible && screen.should_show_actions() {
            layout = layout.push(self.actions.view());
        }
        layout = layout.push(content);

        let outside = Container::new(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(10);

        return outside.into();
        // return mock_view(&mut self.actions);

        // let mut layout = Column::new()
        //     .width(Length::Fill)
        //     .height(Length::Shrink)
        //     .size(30)
        //     .color(text_colour())
        //     .horizontal_alignment(HorizontalAlignment::Left)
        //     .vertical_alignment(VerticalAlignment::Top)
        //     .height(Length::Fill)
        //     .padding(10)
        //     .push(style::title())
        //     .push(Space::new(Length::Fill, Length::Units(10)));
        //
        // if screen.should_show_actions() {
        //     layout = layout
        //         .push(actions.view())
        //         .push(Space::new(Length::Fill, Length::Units(10)));
        // }
        //
        //
        // // Force full width of the content. Height should be default for scrolling to work.
        // let content = Container::new(content).width(Length::Fill);
        // let content = Scrollable::new(scroll_state).push(content);
        // layout.push(content).into()
    }

    fn background_color(&self) -> Color {
        style::background_color().into()
    }
}

// TODO: Result<Manager> (not anyhow::Result because Message needs to be Clone)
async fn prepare_manager() -> Manager {
    Manager::new().unwrap() // TODO: unwrap
}

#[derive(Default, Debug, Clone)]
struct Actions {
    start_button_state: button::State,
    settings_button_state: button::State,
}

impl Actions {
    fn view(&mut self) -> Element<Message> {
        let start_button = Button::new(
            &mut self.start_button_state,
            button_row(Some(steam_icon(20)), Some("Play")),
        )
        .on_press(Message::PlayCiv)
        .style(ActionButtonStyle);

        let right_button = Button::new(
            &mut self.settings_button_state,
            button_row(Some(cog_icon(20)), None),
        )
        .on_press(Message::SetSettingsVisibility(true))
        .style(ActionButtonStyle);

        // let content: Element<Self::Message> = if let Some(err) = &self.err {
        //     Text::new(format!("Error: {:?}", err)).into()
        // } else {
        //     if self. == Yes {
        //         // games_view(&self.manager)
        //         Text::new("").into()
        //     } else if self.has_auth_key == No {
        //         let message = Text::new("no auth key pls enter");
        //         let input = TextInput::new(
        //             &mut self.auth_key_input_state,
        //             "Type something...",
        //             &self.auth_key_input_value,
        //             Message::AuthKeyInputChanged,
        //         )
        //         .padding(10)
        //         .size(20);
        //         let status = Text::new("Updating...")
        //             .vertical_alignment(VerticalAlignment::Center)
        //             .horizontal_alignment(HorizontalAlignment::Center);
        //
        //         // let settings_button: Button<Message> =
        //         //     Button::new(&mut self.settings_button_state, hmm.into()).into();
        //
        //     }
        // };

        let status = Text::new("testing");

        Row::new()
            .height(Length::Units(40))
            .push(start_button.width(Length::Shrink))
            .push(status.width(Length::Fill))
            .push(right_button.width(Length::Shrink))
            .into()
    }
}

#[derive(Default, Debug)]
struct Games {}

impl Games {
    fn view(&mut self, games: &[GameInfo]) -> Element<Message> {
        let mut column = Column::new();
        for info in games {
            let el = Self::game(info.clone());
            column = column.push(el)
        }
        column.into()
    }

    /*
    +------+-------------------------+------------|
    | [     ] | Title of the Game    | [ Upload ] |
    | [     ] | 5d 2h left, 2d5h ago |            |
    | [     ] | [ ] [ ] [ ] [ ]      |            |
    +------+-------------------------+------------|
     */
    fn game(info: GameInfo) -> Element<'static, Message> {
        Row::new()
            .push(Self::avatar(info.clone()))
            .push(Self::title_and_players(info.clone()))
            .push(Self::actions(info.clone()))
            .into()
    }

    fn avatar(info: GameInfo) -> Element<'static, Message> {
        Text::new("AVATAR").width(Length::Units(50)).into()
    }
    fn title_and_players(info: GameInfo) -> Element<'static, Message> {
        Column::new()
            .push(Text::new(info.game.name))
            .push(Text::new("PLAYERS PLAYER PLAYERS"))
            .width(Length::Fill)
            .into()
    }
    fn actions(info: GameInfo) -> Element<'static, Message> {
        Text::new("ACTIONS").into()
    }
}
