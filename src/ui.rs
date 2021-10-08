use crate::ui::HasAuthKey::{No, Yes};
use civfun_gmr::api::{Game, GetGamesAndPlayers, Player};
use civfun_gmr::manager::{Config, Manager};
use iced::container::{Style, StyleSheet};
use iced::window::Mode;
use iced::{
    button, container, executor, scrollable, text_input, time, window, Application, Background,
    Button, Clipboard, Color, Column, Command, Container, Element, HorizontalAlignment, Length,
    Row, Scrollable, Settings, Subscription, Text, TextInput, VerticalAlignment,
};
use tokio::time::Instant;
use tracing::{debug, error, info, instrument, warn};

const TITLE: &str = "civ.fun's Multiplayer Robot";

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
enum HasAuthKey {
    Loading,
    Yes,
    No,
}

impl Default for HasAuthKey {
    fn default() -> Self {
        HasAuthKey::Loading
    }
}

#[derive(Default)]
pub struct CivFunUi {
    err: Option<anyhow::Error>,

    manager: Option<Manager>,
    has_auth_key: HasAuthKey,

    games: Vec<Game>,
    players: Vec<Player>,

    auth_key_input_state: text_input::State,
    auth_key_input_value: String,
    auth_key_button: button::State,

    scroll: scrollable::State,
    refresh_started_at: Option<Instant>,
}

#[derive(Debug, Clone)]
pub enum Message {
    ManagerLoaded(Manager),
    RequestRefresh,
    HasRefreshed,
    AuthKeyInputChanged(String),
    AuthKeySave,
}

impl CivFunUi {
    fn text_colour(&self) -> Color {
        Color::from_rgb(0.9, 0.9, 1.0)
    }
}

// TODO: Return Result<> (not anyhow::Result)
async fn fetch(manager: &mut Manager) {
    manager.refresh().await.unwrap() // TODO: unwrap
}

#[instrument]
fn fetch_cmd(manager: &Option<Manager>) -> Command<Message> {
    debug!("Attempt to fetch.");
    if let Some(ref manager) = manager {
        let mut manager = manager.clone();
        // TODO: unwrap
        if manager.has_auth_key().unwrap() {
            return Command::perform(
                async move { fetch(&mut manager).await },
                Message::HasRefreshed,
            );
        }
    }

    warn!("Manager not set while trying to fetch.");
    Command::none()
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
            ManagerLoaded(manager) => {
                debug!("ManagerLoaded");
                // TODO: unwrap
                let has_auth_key = manager.has_auth_key().unwrap();
                self.manager = Some(manager);

                if has_auth_key {
                    debug!("☑ Has auth key.");
                    self.has_auth_key = Yes;
                    return fetch_cmd(&self.manager);
                } else {
                    debug!("Does not have auth key.");
                    self.has_auth_key = No;
                }
            }
            // ManagerLoaded(Err(e)) => {
            //     self.err = Some(e);
            // }
            RequestRefresh => {
                debug!("RequestRefresh");
                return fetch_cmd(&self.manager);
            }
            HasRefreshed => {
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
                if let Some(ref manager) = self.manager {
                    // TODO: unwrap
                    manager.set_auth_key(&self.auth_key_input_value).unwrap();
                    // Clear the data since the user might have changed auth keys.
                    manager.clear_data().unwrap(); // TODO: unwrap
                    self.has_auth_key = Yes;
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
        time::every(std::time::Duration::from_secs(10)).map(|_| Message::RequestRefresh)
    }

    fn view(&mut self) -> Element<Self::Message> {
        let title = Text::new(TITLE)
            .width(Length::Fill)
            .height(Length::Shrink)
            .size(30)
            .color(self.text_colour())
            .horizontal_alignment(HorizontalAlignment::Left)
            .vertical_alignment(VerticalAlignment::Top);

        let content: Element<Self::Message> = if let Some(err) = &self.err {
            Text::new(format!("Error: {:?}", err)).into()
        } else {
            if self.has_auth_key == Yes {
                games_view(&self.manager)
            } else if self.has_auth_key == No {
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
            } else {
                Text::new("Loading...").into()
            }
        };
        let content: Container<Self::Message> = Container::new(content).into();
        let scrollable: Element<Self::Message> = Scrollable::new(&mut self.scroll)
            .width(Length::Fill)
            .height(Length::Fill)
            .push(content)
            .into();

        let layout: Element<Self::Message> = Column::new().push(title).push(scrollable).into();

        Container::new(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(10)
            // .style(Dark)
            .into()
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
    c = c.push(Text::new("ASDF"));
    c = c.push(Text::new("ASDF2"));
    for game in manager.games() {
        c = c.push(Text::new(format!("ASDF{}", &game.name)));
    }
    c.into()
}
