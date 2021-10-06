use anyhow::Context;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub struct Api {
    auth_key: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetGamesAndPlayers {
    pub games: Vec<Game>,
    pub players: Vec<Player>,
    pub current_total_points: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Game {
    pub name: String,
    pub game_id: i64,
    pub players: Vec<PlayerOrder>,
    pub current_turn: CurrentTurn,
    #[serde(rename = "Type")]
    pub typ: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PlayerOrder {
    pub user_id: i64,
    pub turn_order: i64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CurrentTurn {
    pub turn_id: i64,
    pub number: i64,
    pub user_id: i64,
    pub started: String,
    pub expires: Option<String>,
    pub skipped: bool,
    pub player_number: i64,
    pub is_first_turn: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Player {
    #[serde(rename = "SteamID")]
    pub steam_id: i64,
    pub persona_name: String,
    pub avatar_url: String,
    pub persona_state: i64,
    #[serde(rename = "GameID")]
    pub game_id: i64,
}

impl Api {
    pub fn new(auth_key: &str) -> Self {
        Self {
            auth_key: auth_key.to_owned(),
        }
    }

    async fn get<T>(&self, endpoint: &str, extra_query: &[(&str, &str)]) -> anyhow::Result<T>
    where
        T: DeserializeOwned,
    {
        let client = reqwest::Client::new();
        let mut query = vec![];
        query.push(("authKey", self.auth_key.as_str()));
        query.extend_from_slice(extra_query);
        let resp = client
            .get(format!(
                "http://multiplayerrobot.com/api/Diplomacy/{}",
                endpoint
            ))
            .query(&query)
            .send()
            .await?;
        let text = resp.text().await?;
        Ok(serde_json::from_str(&text).with_context(|| {
            format!(
                "Endpoint: {} ExtraQuery: {:?} JSON: {}",
                endpoint, extra_query, text,
            )
        })?)
    }

    pub async fn get_games_and_players(
        &self,
        player_ids: &[&str],
    ) -> anyhow::Result<GetGamesAndPlayers> {
        let player_id_text = player_ids.join("_");
        self.get("GetGamesAndPlayers", &[("playerIDText", &player_id_text)])
            .await
    }
}
