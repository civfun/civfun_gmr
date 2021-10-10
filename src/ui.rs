use civfun_gmr::api::{Game, GetGamesAndPlayers, Player};
use civfun_gmr::manager::{AuthState, Config, Manager};
use iced::container::{Style, StyleSheet};
use iced::window::Mode;
use iced::{
    button, container, executor, scrollable, text_input, time, window, Align, Application,
    Background, Button, Clipboard, Color, Column, Command, Container, Element, Font,
    HorizontalAlignment, Length, Row, Rule, Scrollable, Settings, Space, Subscription, Text,
    TextInput, VerticalAlignment,
};
use tokio::time::Instant;
use tracing::{debug, error, info, instrument, warn};

use crate::{style, TITLE, VERSION};

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
    Settings,
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

    err: Option<anyhow::Error>,
    status_text: String,
    manager: Option<Manager>,

    actions: Actions,
    enter_auth_key: EnterAuthKey,

    scroll_state: scrollable::State,
    refresh_started_at: Option<Instant>,
}

#[derive(Debug, Clone)]
pub enum Message {
    ManagerLoaded(Manager),
    AuthResponse(Option<u64>),
    RequestRefresh,
    HasRefreshed(()),
    AuthKeyInputChanged(String),
    AuthKeySave,
    PlayCiv,
    ShowSettings,
}

// TODO: Return Result<> (not anyhow::Result)
async fn fetch(manager: &mut Manager) {
    manager.refresh().await.unwrap(); // TODO: unwrap
}

#[instrument(skip(manager))]
fn fetch_cmd(manager: &Option<Manager>) -> Command<Message> {
    debug!("Attempt to fetch.");
    if let Some(ref manager) = manager {
        let mut manager = manager.clone();
        if manager.auth_ready() {
            return Command::perform(
                async move {
                    fetch(&mut manager).await;
                },
                Message::HasRefreshed,
            );
        }
    }

    warn!("Manager not set while trying to fetch.");
    Command::none()
}

async fn authenticate(mut manager: Manager) -> Option<u64> {
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
            HasRefreshed(()) => {
                debug!("HasRefreshed");
                self.status_text = "".into();

                // info!("Got games!!! {:?}", data);
                // self.games = data.games;
                // self.players = data.players;
                // info!("games len {}", self.games.len());
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
                    manager.clear_data().unwrap(); // TODO: unwrap
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
            ShowSettings => self.screen = Screen::Settings,
        }
        Command::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        time::every(std::time::Duration::from_secs(60)).map(|_| Message::RequestRefresh)
    }

    fn view(&mut self) -> Element<Self::Message> {
        // TODO: Turn content to scrollable

        let Self {
            manager,
            screen,
            actions,
            scroll_state,
            enter_auth_key,
            ..
        } = self;

        let mut layout = Column::new()
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(10)
            .push(style::title())
            .push(Space::new(Length::Fill, Length::Units(10)));

        if screen.should_show_actions() {
            layout = layout
                .push(actions.view())
                .push(Space::new(Length::Fill, Length::Units(10)));
        }

        let content: Element<Message> = match screen {
            Screen::NothingYet => Text::new("Something funny is going on!").into(),
            Screen::AuthKeyInput => enter_auth_key.view().into(),
            Screen::Games => Text::new("g\na\n\n\n\n\nmes\n\n\n\n\n li\nst").into(),
            Screen::Settings => Text::new("Settings").into(),
            Screen::Error(msg) => Text::new(format!("Error!\n\n{}", msg)).into(),
        };

        // Force full width of the content. Height should be default for scrolling to work.
        let content = Container::new(content).width(Length::Fill);
        let content = Scrollable::new(scroll_state).push(content);
        layout.push(content).into()
    }

    fn background_color(&self) -> Color {
        style::background_color().into()
    }
}

// TODO: Result<Manager> (not anyhow::Result because Message needs to be Clone)
async fn prepare_manager() -> Manager {
    Manager::new().unwrap() // TODO: unwrap
}

fn games_view<'a>(manager: &Manager) -> Element<Message> {
    let mut c = Column::new();
    for game in manager.games() {
        c = c.push(Text::new(format!("{}", &game.name)));
    }
    c.into()
}

fn content() -> Element<'static, Message> {
    Text::new("content").into()
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

        let status = Text::new("Updating...")
            .vertical_alignment(VerticalAlignment::Center)
            .horizontal_alignment(HorizontalAlignment::Center);

        let settings_button = Button::new(
            &mut self.settings_button_state,
            style::button_row(Some(style::cog_icon(20)), None),
        )
        .on_press(Message::ShowSettings)
        .style(ActionButtonStyle);

        // let settings_button: Button<Message> =
        //     Button::new(&mut self.settings_button_state, hmm.into()).into();

        Row::new()
            .height(Length::Units(40))
            .push(start_button.width(Length::Shrink))
            .push(status.width(Length::Fill))
            .push(settings_button.width(Length::Shrink))
            .into()
    }
}

struct ActionButtonStyle;

impl ActionButtonStyle {
    fn base() -> button::Style {
        button::Style {
            background: Some(style::black_25alpha().into()),
            text_color: Color::WHITE,
            ..Default::default()
        }
    }
}

impl button::StyleSheet for ActionButtonStyle {
    fn active(&self) -> button::Style {
        Self::base()
    }

    fn hovered(&self) -> button::Style {
        button::Style {
            background: Some(style::black().into()),
            ..Self::base()
        }
    }

    fn pressed(&self) -> button::Style {
        button::Style {
            background: Some(style::black_50alpha().into()),
            ..Self::base()
        }
    }

    fn disabled(&self) -> button::Style {
        button::Style {
            background: Some(style::grey_50alpha().into()),
            ..Self::base()
        }
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
}
