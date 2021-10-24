use iced::{Column, Element, Length, Row, Text};

use civfun_gmr::manager::GameInfo;

use crate::ui::Message;

#[derive(Default, Debug)]
pub struct GamesList {}

impl GamesList {
    pub fn view(&mut self, games: &[GameInfo]) -> Element<Message> {
        let mut column = Column::new();
        for info in games {
            let el = Self::game(info.clone());
            column = column.push(el)
        }
        column.into()
    }

    /*
    +------+-------------------------+------------|
    | [     ] | Title of the Game    | [ Upload ] |
    | [     ] | 5d 2h left, 2d5h ago |            |
    | [     ] | [ ] [ ] [ ] [ ]      |            |
    +------+-------------------------+------------|
     */
    fn game(info: GameInfo) -> Element<'static, Message> {
        Row::new()
            .push(Self::avatar(info.clone()))
            .push(Self::title_and_players(info.clone()))
            .push(Self::actions(info.clone()))
            .into()
    }

    fn avatar(info: GameInfo) -> Element<'static, Message> {
        Text::new("AVATAR").width(Length::Units(50)).into()
    }
    fn title_and_players(info: GameInfo) -> Element<'static, Message> {
        Column::new()
            .push(Text::new(info.game.name))
            .push(Text::new("PLAYERS PLAYER PLAYERS"))
            .width(Length::Fill)
            .into()
    }
    fn actions(info: GameInfo) -> Element<'static, Message> {
        Text::new("ACTIONS").into()
    }
}
