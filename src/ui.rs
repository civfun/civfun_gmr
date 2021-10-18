use crate::style::{title, ActionButtonStyle};
use crate::{style, TITLE, VERSION};
use civfun_gmr::api::{Game, GetGamesAndPlayers, Player, UserId};
use civfun_gmr::manager::{AuthState, Config, GameInfo, Manager};
use iced::container::{Style, StyleSheet};
use iced::svg::Handle;
use iced::window::Mode;
use iced::{
    button, container, executor, scrollable, text_input, time, window, Align, Application,
    Background, Button, Clipboard, Color, Column, Command, Container, Element, Font,
    HorizontalAlignment, Length, Row, Rule, Scrollable, Settings, Space, Subscription, Svg, Text,
    TextInput, VerticalAlignment,
};
use notify::DebouncedEvent;
use tokio::task::spawn_blocking;
use tokio::time::Instant;
use tracing::{debug, error, info, instrument, warn};

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

#[derive(PartialEq)]
enum Screen {
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

#[derive(Default)]
pub struct CivFunUi {
    screen: Screen,
    // settings_visible is not part of Screen so that screen can change while the settings are showing.
    settings_visible: bool,

    err: Option<anyhow::Error>,
    status_text: String,
    manager: Option<Manager>,

    actions: Actions,
    settings: UiSettings,
    enter_auth_key: EnterAuthKey,
    games: Games,

    scroll_state: scrollable::State,
    refresh_started_at: Option<Instant>,
}

#[derive(Debug, Clone)]
pub enum Message {
    ManagerLoaded(Manager),
    AuthResponse(Option<UserId>),
    RequestRefresh,
    HasRefreshed(()),
    StartedWatching(()),
    ProcessTransfers,
    ProcessNewSaves,
    AuthKeyInputChanged(String),
    AuthKeySave,
    PlayCiv,
    ShowSettings,
    HideSettings,
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
            AuthResponse(Some(_)) => {
                debug!("Authenticated");
                self.screen = Screen::Games;
            }
            AuthResponse(None) => {
                debug!("Bad authentication");
                self.screen = Screen::Error("Bad authentication".into());
            }
            RequestRefresh => {
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

                // info!("Got games!!! {:?}", data);
                // self.games = data.games;
                // self.players = data.players;
                // info!("games len {}", self.games.len());
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
            // Refreshed(Err(err)) => {
            //     error!("error: {:?}", err);
            // }
            AuthKeyInputChanged(s) => {
                self.enter_auth_key.input_value = s;
            }
            AuthKeySave => {
                if let Some(ref mut manager) = self.manager {
                    // TODO: unwrap
                    manager
                        .save_auth_key(&self.enter_auth_key.input_value.trim())
                        .unwrap();
                    // Clear the data since the user might have changed auth keys.
                    manager.clear_games().unwrap(); // TODO: unwrap
                    debug!("Saved auth key and reset data.");
                    self.status_text = "Refreshing...".into(); // TODO: make a fn for these two.
                    self.screen = Screen::Games;
                    return fetch_cmd(&self.manager);
                } else {
                    error!("Manager not initialised while trying to save auth_key.");
                }
            }
            PlayCiv => {
                // TODO: DX version from settings.
                open::that("steam://rungameid/8930//%5Cdx9").unwrap(); // TODO: unwrap
            }
            ShowSettings => self.settings_visible = true,
            CloseSettings => self.settings_visible = false,
        }
        Command::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            time::every(std::time::Duration::from_secs(60)).map(|_| Message::RequestRefresh),
            time::every(std::time::Duration::from_millis(1000)).map(|_| Message::ProcessTransfers),
            time::every(std::time::Duration::from_millis(1000)).map(|_| Message::ProcessNewSaves),
        ])
    }

    fn view(&mut self) -> Element<Self::Message> {
        let Self {
            manager,
            screen,
            actions,
            settings,
            settings_visible,
            scroll_state,
            enter_auth_key,
            games,
            ..
        } = self;

        let mut content = match screen {
            Screen::NothingYet => Text::new("Something funny is going on!").into(),
            Screen::AuthKeyInput => enter_auth_key.view(),
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

#[derive(Default)]
struct Actions {
    start_button_state: button::State,
    settings_button_state: button::State,
}

impl Actions {
    fn view(&mut self) -> Element<Message> {
        let start_button = Button::new(
            &mut self.start_button_state,
            style::button_row(Some(style::steam_icon(20)), Some("Play")),
        )
        .on_press(Message::PlayCiv)
        .style(ActionButtonStyle);

        let right_button = Button::new(
            &mut self.settings_button_state,
            style::button_row(Some(style::cog_icon(20)), None),
        )
        .on_press(Message::ShowSettings)
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

#[derive(Default)]
struct Games {}

impl Games {
    fn view(&mut self, games: &[GameInfo]) -> Element<Message> {
        let column = Column::new();
        for info in games {
            // column.push(Text::new(info.game.name.clone()));
        }
        column.into()
    }
}

#[derive(Default)]
struct EnterAuthKey {
    input_state: text_input::State,
    input_value: String,
    button_state: button::State,
}

impl EnterAuthKey {
    pub fn view(&mut self) -> Element<Message> {
        let message = style::normal_text("Please enter your Authentication Key below.")
            .horizontal_alignment(HorizontalAlignment::Center);

        let input = TextInput::new(
            &mut self.input_state,
            "",
            &self.input_value,
            Message::AuthKeyInputChanged,
        )
        .padding(10)
        .size(20);

        let button = Button::new(
            &mut self.button_state,
            Text::new("Save")
                .height(Length::Fill)
                .vertical_alignment(VerticalAlignment::Center),
        )
        .on_press(Message::AuthKeySave);

        Column::new()
            .align_items(Align::Center)
            .push(Space::new(Length::Fill, Length::Units(50)))
            .push(message)
            .push(Space::new(Length::Fill, Length::Units(10)))
            .push(
                Row::new()
                    .max_width(250)
                    .height(Length::Units(40))
                    .push(input)
                    .push(button.height(Length::Fill)),
            )
            .into()
    }

    fn background_color(&self) -> Color {
        Color::from_rgb(0.168, 0.243, 0.313).into()
    }
}

#[derive(Default)]
struct UiSettings {
    close_settings_button_state: button::State,
    open_folder_button_state: button::State,
}

impl UiSettings {
    fn view(&mut self) -> Element<Message> {
        let close_button = Button::new(
            &mut self.close_settings_button_state,
            style::button_row(Some(style::done_icon(20)), Some("Done")),
        )
        .on_press(Message::HideSettings)
        .style(ActionButtonStyle);

        close_button.into()
    }
}
