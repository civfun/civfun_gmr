use iced::{button, Button, Element, HorizontalAlignment, Length, Row, Text, VerticalAlignment};

use crate::ui::style::{
    action_button, cog_icon, normal_text, steam_icon, ActionButtonStyle, ButtonView,
    NORMAL_ICON_SIZE, ROW_HEIGHT,
};
use crate::ui::Message;

#[derive(Default, Debug, Clone)]
pub struct Actions {
    start_button_state: button::State,
}

impl Actions {
    pub fn view(&mut self) -> Element<Message> {
        // let start_button = Button::new(
        //     &mut self.start_button_state,
        //     button_row(Some(steam_icon(20)), Some("Play")),
        // )
        // .on_press(Message::PlayCiv)
        // .style(ActionButtonStyle);
        let mut start_button = action_button(
            ButtonView::TextIcon("Play", steam_icon(NORMAL_ICON_SIZE)),
            Message::PlayCiv,
            &mut self.start_button_state,
        );

        let status = normal_text("testing").vertical_alignment(VerticalAlignment::Center);

        Row::new()
            .height(Length::Units(ROW_HEIGHT))
            .push(start_button.width(Length::Shrink))
            .push(status.width(Length::Fill))
            .into()
    }
}
