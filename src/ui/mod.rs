use crate::ui::auth_key_screen::AuthKeyMessage;
use crate::ui::style::{action_button, ButtonView, NORMAL_ICON_SIZE};
use crate::{TITLE, VERSION};
use actions::Actions;
use auth_key_screen::AuthKeyScreen;
use civfun_gmr::api::{Game, GetGamesAndPlayers, Player, UserId};
use civfun_gmr::manager::{Event, Manager};
use error_screen::ErrorScreen;
use games_list::GamesList;
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
use prefs::Prefs;
use style::{cog_icon, done_icon, normal_text, steam_icon, title, ActionButtonStyle, ROW_HEIGHT};
use tokio::task::spawn_blocking;
use tokio::time::Instant;
use tracing::{debug, error, info, instrument, trace, warn};

mod actions;
mod auth_key_screen;
mod error_screen;
mod games_list;
mod prefs;
mod style;

pub fn run(manager: Manager) -> anyhow::Result<()> {
    let settings = Settings {
        window: window::Settings {
            size: (400, 400),
            min_size: Some((400, 200)),
            ..Default::default()
        },
        flags: manager,
        default_font: Default::default(),
        default_text_size: 20,
        exit_on_close_request: true,
        antialiasing: true,
    };
    CivFunUi::run(settings)?;
    Ok(())
}

#[derive(PartialEq, Debug, Clone)]
pub enum Screen {
    NothingYet,
    Error { message: String, next: Box<Screen> },
    AuthKeyInput,
    Games,
    Settings,
}

impl Screen {
    pub fn should_show_actions(&self) -> bool {
        match self {
            Screen::Games => true,
            Screen::Error { .. } => true,
            _ => false,
        }
    }
}

impl Default for Screen {
    fn default() -> Self {
        Screen::NothingYet
    }
}

#[derive(Debug)]
pub struct CivFunUi {
    manager: Manager,
    games: Vec<Game>,

    screen: Screen,
    status_text: String,
    settings_button_state: button::State,

    actions: Actions,
    error: ErrorScreen,
    prefs: Prefs,
    enter_auth_key: AuthKeyScreen,
    games_list: GamesList,

    scroll_state: scrollable::State,
}

#[derive(Debug, Clone)]
pub enum Message {
    GetManagerEvents,
    SetScreen(Screen),
    RequestRefresh,
    PlayCiv,

    AuthKeyMessage(AuthKeyMessage),
    AuthKeySave(String),
}

impl Application for CivFunUi {
    type Executor = executor::Default;
    type Message = Message;
    type Flags = Manager;

    fn new(manager: Manager) -> (CivFunUi, Command<Self::Message>) {
        let mut civfun = CivFunUi {
            manager,
            games: vec![],
            screen: Default::default(),
            status_text: "".to_string(),
            error: Default::default(),
            actions: Default::default(),
            prefs: Default::default(),
            enter_auth_key: Default::default(),
            games_list: Default::default(),
            scroll_state: Default::default(),
            settings_button_state: Default::default(),
        };

        if civfun.manager.auth_key().unwrap().is_some() {
            // civfun.status_text = "Refreshing...".into();
            // return Command::batch([
            //     // fetch_cmd(&Some(manager.clone())),
            //     // watch_cmd(&Some(manager.clone())),
            //     // Command::perform(authenticate(manager.clone()), AuthResponse),
            // ]);
            civfun.screen = Screen::Games;
        } else {
            civfun.screen = Screen::AuthKeyInput;
        }

        (civfun, Command::none())
    }

    fn title(&self) -> String {
        format!("{} v{}", TITLE, VERSION)
    }

    #[instrument(skip(self, _clipboard))]
    fn update(
        &mut self,
        message: Self::Message,
        _clipboard: &mut Clipboard,
    ) -> Command<Self::Message> {
        use Message::*;
        match message {
            GetManagerEvents => {
                for event in self.manager.process().unwrap() {
                    trace!(?event);
                    match event {
                        Event::AuthenticationSuccess => {
                            self.status_text = "Authentication Successful".to_string();
                        }
                        Event::AuthenticationFailure => {
                            self.screen = Screen::Error {
                                message: "Authentication Key error".to_string(),
                                next: Box::new(Screen::AuthKeyInput),
                            };
                        }
                        Event::UpdatedGames(games) => {
                            self.games = games;
                        }
                        x => todo!("{:?}", x),
                    }
                }
            }

            AuthKeyMessage(message) => return self.enter_auth_key.update(message, _clipboard),

            AuthKeySave(auth_key) => {
                self.screen = Screen::Games;
                self.status_text = "Authenticating".to_string();
                self.manager.authenticate(&auth_key).unwrap();
            }

            SetScreen(screen) => {
                self.screen = screen;
            }
            RequestRefresh => {
                debug!("RequestRefresh");
                todo!();
                self.status_text = "Refreshing...".into();
                // return fetch_cmd(&self.manager);
            }
            PlayCiv => {
                // TODO: DX version from settings.
                open::that("steam://rungameid/8930//%5Cdx9").unwrap(); // TODO: unwrap
            }
        }
        Command::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            time::every(std::time::Duration::from_secs(60)).map(|_| Message::RequestRefresh),
            time::every(std::time::Duration::from_millis(100)).map(|_| Message::GetManagerEvents),
        ])
    }

    fn view(&mut self) -> Element<Self::Message> {
        let Self {
            manager,
            screen,
            error,
            actions,
            prefs: settings,
            scroll_state,
            enter_auth_key,
            games_list,
            ref mut settings_button_state,
            ..
        } = self;

        let mut content = match screen {
            Screen::NothingYet => normal_text("Loading...").into(),
            Screen::AuthKeyInput => enter_auth_key.view().map(Message::AuthKeyMessage),
            Screen::Games => games_list.view(&self.games),
            Screen::Settings => settings.view(),
            Screen::Error {
                message: text,
                next,
            } => error.view(&text, *next.clone()),
        };

        // // TODO: Turn content to scrollable
        // let content = Scrollable::new(&mut scroll)
        //     .width(Length::Fill)
        //     .height(Length::Fill)
        //     .push(content);
        //
        // let settings_button = Button::new(
        //     settings_button_state,
        //     button_row(ButtonView::Icon(cog_icon(NORMAL_ICON_SIZE))),
        // )
        // .on_press(Message::SetScreen(Screen::Settings))
        // .style(ActionButtonStyle);

        let settings_button = action_button(
            ButtonView::Icon(cog_icon(NORMAL_ICON_SIZE)),
            Message::SetScreen(Screen::Settings),
            settings_button_state,
        );

        let title_row = Row::new()
            .height(Length::Units(ROW_HEIGHT))
            .push(title())
            .push(settings_button);

        let actions = if screen.should_show_actions() {
            actions.view()
        } else {
            Space::new(Length::Shrink, Length::Shrink).into()
        };

        let layout = Column::new().push(title_row).push(actions).push(content);

        let outside = Container::new(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(10);

        let hmm: Element<Self::Message> = outside.into();
        hmm.explain(Color::WHITE)

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
