use iced::{Column, Element, Length, Row, Text};

use crate::ui::Message;
use civfun_gmr::api::Game;

#[derive(Default, Debug)]
pub struct GamesList {}

impl GamesList {
    pub fn view(&mut self, games: &[Game]) -> Element<Message> {
        let mut column = Column::new();
        for game in games {
            let el = Self::game(game.clone());
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
    fn game(game: Game) -> Element<'static, Message> {
        Row::new()
            .push(Self::avatar(game.clone()))
            .push(Self::title_and_players(game.clone()))
            .push(Self::actions(game.clone()))
            .into()
    }

    fn avatar(info: Game) -> Element<'static, Message> {
        Text::new("AVATAR").width(Length::Units(50)).into()
    }
    fn title_and_players(game: Game) -> Element<'static, Message> {
        Column::new()
            .push(Text::new(game.name))
            .push(Text::new("PLAYERS PLAYER PLAYERS"))
            .width(Length::Fill)
            .into()
    }
    fn actions(info: Game) -> Element<'static, Message> {
        Text::new("ACTIONS").into()
    }
}
