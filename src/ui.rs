use civfun_gmr::{Config, Manager};
use iced::container::{Style, StyleSheet};
use iced::{
    container, executor, scrollable, window, Application, Background, Clipboard, Color, Column,
    Command, Container, Element, HorizontalAlignment, Length, Scrollable, Settings, Text,
    VerticalAlignment,
};
use tracing::info;

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

#[derive(Default)]
pub struct CivFunUi {
    manager: Option<Manager>,
    config: Option<Config>,
    err: Option<anyhow::Error>,
    scroll: scrollable::State,
}

#[derive(Debug)]
pub enum Message {
    ManagerLoaded(anyhow::Result<(Manager, Config)>),
}

impl CivFunUi {
    fn text_colour(&self) -> Color {
        Color::from_rgb(0.9, 0.9, 1.0)
    }

    fn view_games(&self) -> Element<Message> {
        Text::new("Hello\n\n\n\nhmmm\n\n\n, world\n\n\n\n\n\n\n\n33\n\n\n\n3\nn\n\n\n\n!").into()
        // let content = Column::new().max_width(800).spacing(20).push(title);
    }
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

    fn update(
        &mut self,
        message: Self::Message,
        _clipboard: &mut Clipboard,
    ) -> Command<Self::Message> {
        use Message::*;
        match message {
            ManagerLoaded(Ok((m, c))) => {
                self.manager = Some(m);
                self.config = Some(c);
            }
            ManagerLoaded(Err(e)) => {
                self.err = Some(e);
            }
        }
        Command::none()
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
            self.view_games()
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
            .style(Dark)
            .into()
    }
}

async fn prepare_manager() -> anyhow::Result<(Manager, Config)> {
    let manager = Manager::new()?;
    let config = manager.get_or_create_config()?;
    Ok((manager, config))
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
