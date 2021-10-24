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
use tracing::{debug, error, info, instrument, trace, warn};

use actions::Actions;
use auth_key_screen::AuthKeyScreen;
use civfun_gmr::api::{Game, GetGamesAndPlayers, Player, UserId};
use civfun_gmr::manager::{AuthState, Config, Event, GameInfo, Manager};
use error_screen::ErrorScreen;
use games_list::GamesList;
use prefs::Prefs;
use style::{cog_icon, done_icon, normal_text, steam_icon, title, ActionButtonStyle, ROW_HEIGHT};

use crate::ui::auth_key_screen::AuthKeyMessage;
use crate::ui::style::{action_button, ButtonView, NORMAL_ICON_SIZE};
use crate::{TITLE, VERSION};

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
    screen: Screen,

    status_text: String,
    manager: Manager,

    settings_button_state: button::State,

    actions: Actions,
    error: ErrorScreen,
    prefs: Prefs,
    enter_auth_key: AuthKeyScreen,
    games: GamesList,

    scroll_state: scrollable::State,
}

#[derive(Debug, Clone)]
pub enum Message {
    GetManagerEvents,
    SetScreen(Screen),
    RequestRefresh(()),
    HasRefreshed(()),
    StartedWatching(()),
    ProcessTransfers,
    ProcessNewSaves,
    // AuthKeyInputChanged(String),
    // AuthKeySave,
    PlayCiv,

    AuthKeyMessage(AuthKeyMessage),
    AuthKeySave(String),
}

// // TODO: Return Result<> (not anyhow::Result)
// async fn fetch(mut manager: Manager) {
//     manager.refresh().await.unwrap(); // TODO: unwrap
// }
//
// #[instrument(skip(manager))]
// fn fetch_cmd(manager: &Option<Manager>) -> Command<Message> {
//     debug!("Attempt to fetch.");
//     let manager = match manager {
//         Some(ref m) => m.clone(),
//         None => {
//             warn!("Manager not set while trying to fetch.");
//             return Command::none();
//         }
//     };
//
//     let is_auth_ready = manager.auth_ready();
//     if !is_auth_ready {
//         return Command::none();
//     }
//
//     let mut manager = manager.clone();
//     Command::perform(
//         async {
//             fetch(manager).await;
//         },
//         Message::HasRefreshed,
//     )
// }
//
// #[instrument(skip(manager))]
// fn watch_cmd(manager: &Option<Manager>) -> Command<Message> {
//     let manager = match manager {
//         Some(ref m) => m.clone(),
//         None => {
//             warn!("Manager not set while trying to fetch.");
//             return Command::none();
//         }
//     };
//
//     Command::perform(
//         async move { manager.start_watching_saves().await.unwrap() },
//         Message::StartedWatching,
//     ) // TODO: unwrap
// }

impl Application for CivFunUi {
    type Executor = executor::Default;
    type Message = Message;
    type Flags = Manager;

    fn new(manager: Manager) -> (CivFunUi, Command<Self::Message>) {
        let mut civfun = CivFunUi {
            manager,
            screen: Default::default(),
            status_text: "".to_string(),
            error: Default::default(),
            actions: Default::default(),
            prefs: Default::default(),
            enter_auth_key: Default::default(),
            games: Default::default(),
            scroll_state: Default::default(),
            settings_button_state: Default::default(),
        };

        if civfun.manager.auth_ready() {
            debug!("☑ Has auth key.");
            // civfun.status_text = "Refreshing...".into();
            // return Command::batch([
            //     // fetch_cmd(&Some(manager.clone())),
            //     // watch_cmd(&Some(manager.clone())),
            //     // Command::perform(authenticate(manager.clone()), AuthResponse),
            // ]);
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
                while let Some(event) = self.manager.process().unwrap() {
                    match event {
                        // Event::AuthenticationSuccess => {}
                        Event::AuthenticationFailure => {
                            trace!("yoooooo");
                            self.screen = Screen::Error {
                                message: "Authentication Key error".to_string(),
                                next: Box::new(Screen::AuthKeyInput),
                            };
                        }
                        x => todo!("{:?}", x),
                    }
                }
            }

            AuthKeyMessage(message) => return self.enter_auth_key.update(message, _clipboard),

            AuthKeySave(auth_key) => {
                // if let Some(ref mut manager) = self.manager {
                //     // TODO: unwrap
                //     manager.save_auth_key(&auth_key).unwrap();
                //     self.status_text = "Refreshing...".into(); // TODO: make a fn for these two.
                //     self.screen = Screen::Games;
                //     return Command::batch([
                //         Command::perform(async { Screen::Games }, Message::SetScreen),
                //         Command::perform(async { () }, Message::RequestRefresh),
                //     ]);
                // } else {
                //     error!("Manager not initialised while trying to save auth_key.");
                // }
                self.manager.authenticate(&auth_key);
            }

            SetScreen(screen) => {
                self.screen = screen;
            }
            RequestRefresh(()) => {
                debug!("RequestRefresh");
                todo!();
                self.status_text = "Refreshing...".into();
                // return fetch_cmd(&self.manager);
            }
            StartedWatching(()) => {
                debug!("StartedWatching");
            }
            HasRefreshed(()) => {
                debug!("HasRefreshed");
                self.status_text = "".into();
            }
            ProcessTransfers => {
                // if let Some(ref mut manager) = self.manager {
                //     let mut manager = manager.clone();
                //     tokio::spawn(async move {
                //         manager.process_transfers().await.unwrap();
                //     });
                // }
                todo!()
            }
            ProcessNewSaves => {
                todo!()
                // if let Some(ref mut manager) = self.manager {
                //     manager.process_new_saves().unwrap();
                // }
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
            // time::every(std::time::Duration::from_secs(60)).map(|_| Message::RequestRefresh(())),
            time::every(std::time::Duration::from_millis(1000)).map(|_| Message::GetManagerEvents),
            // time::every(std::time::Duration::from_millis(1000)).map(|_| Message::ProcessNewSaves),
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
            games,
            ref mut settings_button_state,
            ..
        } = self;

        let mut content = match screen {
            Screen::NothingYet => normal_text("Loading...").into(),
            Screen::AuthKeyInput => enter_auth_key.view().map(Message::AuthKeyMessage),
            Screen::Games => games.view(manager.games().as_slice()),
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
