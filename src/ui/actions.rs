use iced::{button, Button, Element, Length, Row, Text};

use crate::ui::style::{button_row, cog_icon, steam_icon, ActionButtonStyle};
use crate::ui::Message;

#[derive(Default, Debug, Clone)]
pub struct Actions {
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

        let status = Text::new("testing");

        Row::new()
            .height(Length::Units(40))
            .push(start_button.width(Length::Shrink))
            .push(status.width(Length::Fill))
            .push(right_button.width(Length::Shrink))
            .into()
    }
}
