use civfun_gmr::api::{Game, GetGamesAndPlayers, Player};
use civfun_gmr::manager::{Config, Manager};
use iced::container::{Style, StyleSheet};
use iced::window::Mode;
use iced::{
    button, container, executor, scrollable, text_input, time, window, Application, Background,
    Button, Clipboard, Color, Column, Command, Container, Element, Font, HorizontalAlignment,
    Length, Row, Rule, Scrollable, Settings, Subscription, Text, TextInput, VerticalAlignment,
};
use tokio::time::Instant;
use tracing::{debug, error, info, instrument, warn};

const TITLE: &str = "civ.fun's Multiplayer Robot";

const ICONS: Font = Font::External {
    name: "Icons",
    bytes: include_bytes!("../fonts/ionicons.ttf"),
};

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

#[derive(Default)]
pub struct CivFunUi {
    err: Option<anyhow::Error>,

    manager: Option<Manager>,

    auth_key_input_state: text_input::State,
    auth_key_input_value: String,
    auth_key_button: button::State,

    scroll: scrollable::State,
    refresh_started_at: Option<Instant>,
}

#[derive(Debug, Clone)]
pub enum Message {
    ManagerLoaded(Manager),
    Authenticated(()),
    RequestRefresh,
    HasRefreshed(()),
    AuthKeyInputChanged(String),
    AuthKeySave,
}

impl CivFunUi {
    fn text_colour(&self) -> Color {
        Color::from_rgb(0.9, 0.9, 1.0)
    }

    fn content(&mut self) -> Element<Message> {
        let content: Element<Message> = if let Some(err) = &self.err {
            Text::new(format!("Error: {:?}", err)).into()
        } else {
            if let Some(manager) = &self.manager {
                if manager.auth_ready() {
                    games_view(manager)
                } else {
                    let message = Text::new("no auth key pls enter");
                    let input = TextInput::new(
                        &mut self.auth_key_input_state,
                        "Type something...",
                        &self.auth_key_input_value,
                        Message::AuthKeyInputChanged,
                    )
                    .padding(10)
                    .size(20);

                    let button = Button::new(&mut self.auth_key_button, Text::new("Save"))
                        .on_press(Message::AuthKeySave); // .on_press

                    Column::new()
                        .push(message)
                        .push(Row::new().push(input).push(button))
                        .into()
                }
            } else {
                Text::new("Loading manager...").into()
            }
        };
        content
    }

    fn ready_view(&mut self) -> Element<Message> {}
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

async fn authenticate(mut manager: Manager) {
    // TODO: unwrap
    manager.authenticate().await.unwrap();
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
        TITLE.into()
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
                self.manager = Some(manager.clone());

                debug!("ManagerLoaded");
                if manager.auth_ready() {
                    debug!("☑ Has auth key.");
                    return Command::batch([
                        fetch_cmd(&Some(manager.clone())),
                        Command::perform(authenticate(manager.clone()), Authenticated),
                    ]);
                }
            }
            Authenticated(()) => {
                debug!("Authenticated");
            }
            // ManagerLoaded(Err(e)) => {
            //     self.err = Some(e);
            // }
            RequestRefresh => {
                debug!("RequestRefresh");
                return fetch_cmd(&self.manager);
            }
            HasRefreshed(()) => {
                debug!("HasRefreshed");
                // info!("Got games!!! {:?}", data);
                // self.games = data.games;
                // self.players = data.players;
                // info!("games len {}", self.games.len());
            }
            // Refreshed(Err(err)) => {
            //     error!("error: {:?}", err);
            // }
            AuthKeyInputChanged(s) => {
                self.auth_key_input_value = s;
            }
            AuthKeySave => {
                if let Some(ref mut manager) = self.manager {
                    // TODO: unwrap
                    manager.save_auth_key(&self.auth_key_input_value).unwrap();
                    // Clear the data since the user might have changed auth keys.
                    manager.clear_data().unwrap(); // TODO: unwrap
                    debug!("Saved auth key and reset data.");
                    return fetch_cmd(&self.manager);
                } else {
                    error!("Manager not initialised while trying to save auth_key.");
                }
            }
        }
        Command::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        time::every(std::time::Duration::from_secs(60)).map(|_| Message::RequestRefresh)
    }

    fn view(&mut self) -> Element<Self::Message> {
        // TODO: Turn content to scrollable
        // let content = Scrollable::new(&mut self.scroll)
        //     .width(Length::Fill)
        //     .height(Length::Fill)
        //     .push(content());

        // let mut actions = Actions::default();

        let layout = Column::new()
            .push(title())
            .push(self.actions.view())
            .push(content());

        let outside = Container::new(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(10);

        outside.into()
        // return mock_view(&mut self.actions);

        // let title = Text::new(TITLE)
        //     .width(Length::Fill)
        //     .height(Length::Shrink)
        //     .size(30)
        //     .color(self.text_colour())
        //     .horizontal_alignment(HorizontalAlignment::Left)
        //     .vertical_alignment(VerticalAlignment::Top);
        //
        // let controls = self.view_controls();
        //
        // let content: Element<Message> = if let Some(err) = &self.err {
        //     Text::new(format!("Error: {:?}", err)).into()
        // } else {
        //     if let Some(manager) = &self.manager {
        //         if manager.auth_ready() {
        //             ready_view(manager)
        //         } else {
        //             enter_auth_key_view(manager)
        //         }
        //     } else {
        //         Text::new("Loading manager...").into()
        //     }
        // };
        //
        // let content: Container<Self::Message> = Container::new(content).into();
        // // let scrollable: Element<Self::Message> = Scrollable::new(&mut self.scroll)
        // //     .width(Length::Fill)
        // //     .height(Length::Fill)
        // //     .push(content)
        // //     .into();
        // let scrollable = content;
        //
        // let layout: Element<Self::Message> = Column::new().push(title).push(scrollable).into();
        //
        // Container::new(layout)
        //     .width(Length::Fill)
        //     .height(Length::Fill)
        //     .padding(10)
        //     // .style(Dark)
        //     .into()
    }

    fn background_color(&self) -> Color {
        Color::from_rgb(0.168, 0.243, 0.313).into()
    }
}

// TODO: Result<Manager> (not anyhow::Result because Message needs to be Clone)
async fn prepare_manager() -> Manager {
    Manager::new().unwrap() // TODO: unwrap
}

struct Dark;

impl container::StyleSheet for Dark {
    fn style(&self) -> Style {
        Style {
            background: Some(Color::from_rgb(0.168, 0.243, 0.313).into()),
            ..Default::default()
        }
    }
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

fn title() -> Element<'static, Message> {
    Text::new(TITLE)
        .width(Length::Fill)
        .height(Length::Shrink)
        .size(30)
        .color(text_colour())
        .horizontal_alignment(HorizontalAlignment::Left)
        .vertical_alignment(VerticalAlignment::Top)
        .into()
}

fn text_colour() -> Color {
    Color::from_rgb(0.9, 0.9, 1.0)
}

#[derive(Default)]
struct Actions {
    start_button_state: button::State,
    settings_button_state: button::State,
}

impl Actions {
    fn view(&mut self) -> Element<Message> {
        let start_button = Button::new(&mut self.start_button_state, Text::new("Play"));
        let status = Text::new("Updating...")
            .vertical_alignment(VerticalAlignment::Center)
            .horizontal_alignment(HorizontalAlignment::Center);

        let hmm = warning_icon();

        let settings_button: Button<Message> =
            Button::new(&mut self.settings_button_state, hmm.into()).into();

        Row::new()
            .push(start_button.width(Length::Shrink))
            .push(status.width(Length::Fill))
            .push(hmm.width(Length::Shrink))
            .into()
    }
}

fn icon(unicode: char) -> Text {
    Text::new(&unicode.to_string())
        .font(ICONS)
        .width(Length::Units(20))
        .horizontal_alignment(HorizontalAlignment::Center)
        .size(20)
}

fn warning_icon() -> Text {
    icon('')
}
