use crate::ui::style::{
    action_button, centered_column, normal_text, title_text, vertically_centered_content,
    ButtonView, RELAXED_PADDING,
};
use crate::ui::{Message, Screen};
use iced::{button, Align, Column, Container, Element, HorizontalAlignment, Length};

#[derive(Debug, Default)]
pub struct ErrorScreen {
    close_button_state: button::State,
}

impl ErrorScreen {
    pub fn view(&mut self, text: &str, next: Screen) -> Element<Message> {
        let title = title_text("Oh no!");
        let message = normal_text(text);
        let close_button = action_button(
            ButtonView::Text("Okay, thanks."),
            Message::SetScreen(next),
            &mut self.close_button_state,
        );

        vertically_centered_content(
            centered_column()
                .push(title)
                .push(message)
                .push(close_button),
        )
        .into()
    }
}
