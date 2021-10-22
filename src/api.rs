use anyhow::{anyhow, Context};
use iced::futures::{Stream, StreamExt};
use reqwest::multipart::{Form, Part};
use reqwest::{Body, Method, RequestBuilder, Response};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::convert::{TryFrom, TryInto};
use std::fmt::{Display, Formatter};
use std::io::{Bytes, Write};
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, TempPath};
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{info, instrument, trace, trace_span};

#[derive(Default, Debug, Copy, Clone, PartialEq, Serialize, Deserialize)]
pub struct UserId(u64);

impl From<u64> for UserId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

impl Display for UserId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Serialize, Deserialize, Hash, Eq)]
pub struct GameId(u32);

impl From<u32> for GameId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}

impl Display for GameId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Default, Debug, Copy, Clone, PartialEq, Serialize, Deserialize, Hash, Eq)]
pub struct TurnId(u64);

impl From<u64> for TurnId {
    fn from(id: u64) -> Self {
        Self(id)
    }
}

impl Display for TurnId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct GetGamesAndPlayers {
    pub games: Vec<Game>,
    pub players: Vec<Player>,
    pub current_total_points: u64,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Game {
    pub name: String,
    pub game_id: GameId,
    pub players: Vec<PlayerOrder>,
    pub current_turn: CurrentTurn,
    #[serde(rename = "Type")]
    pub typ: u8,
}

impl Game {
    pub fn is_user_id_turn(&self, user_id: &UserId) -> bool {
        &self.current_turn.user_id == user_id
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct PlayerOrder {
    pub user_id: UserId,
    pub turn_order: u16,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct CurrentTurn {
    pub turn_id: TurnId,
    pub number: u64,
    pub user_id: UserId,
    pub started: String,
    pub expires: Option<String>,
    pub skipped: bool,
    pub player_number: u64,
    pub is_first_turn: bool,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Player {
    // Not sure if this is right?
    #[serde(rename = "SteamID")]
    pub steam_id: UserId,
    pub persona_name: String,
    pub avatar_url: String,
    pub persona_state: u64,
    #[serde(rename = "GameID")]
    pub game_id: GameId,
}

#[derive(Clone, Debug)]
pub struct Percentage(f32);

impl TryFrom<f32> for Percentage {
    type Error = anyhow::Error;

    fn try_from(value: f32) -> Result<Self, Self::Error> {
        if value < 0.0 || value > 1.0 {
            return Err(anyhow!("Percentage outside of range: {}", value));
        }
        Ok(Percentage(value))
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct UploadResponse {
    result_type: u8,
    points_earned: u32,
}

#[derive(Clone, Debug)]
pub enum DownloadMessage {
    Error(String),
    Started(Option<u64>),
    Chunk(Option<Percentage>),
    Done(PathBuf),
}

#[derive(Clone, Debug)]
pub enum UploadMessage {
    Error(String),
    Started,
    Chunk(Option<Percentage>),
    Done,
}

#[derive(Clone)]
pub struct Api {
    auth_key: String,
}

impl Api {
    pub fn new(auth_key: &str) -> Self {
        Self {
            auth_key: auth_key.to_owned(),
        }
    }

    #[instrument(skip(self))]
    fn query(
        &self,
        method: Method,
        endpoint: &str,
        extra_query: &[(&str, &str)],
    ) -> anyhow::Result<RequestBuilder> {
        let client = reqwest::Client::new();
        let mut query = vec![];
        query.push(("authKey", self.auth_key.as_str()));
        query.extend_from_slice(extra_query);
        let url = format!("http://multiplayerrobot.com/api/Diplomacy/{}", endpoint);
        Ok(client.request(method, url).query(&query))
    }

    #[instrument(skip(self))]
    async fn get(&self, endpoint: &str, extra_query: &[(&str, &str)]) -> anyhow::Result<Response> {
        let req = self.query(Method::GET, endpoint, extra_query)?;
        Ok(req.send().await?)
    }

    #[instrument(skip(self))]
    async fn get_text(
        &self,
        endpoint: &str,
        extra_query: &[(&str, &str)],
    ) -> anyhow::Result<String> {
        let response = self.get(endpoint, extra_query).await?;
        let text = response.text().await?;
        trace!("Response: {}", text);
        Ok(text)
    }

    #[instrument(skip(self))]
    async fn get_json<T>(&self, endpoint: &str, extra_query: &[(&str, &str)]) -> anyhow::Result<T>
    where
        T: DeserializeOwned,
    {
        let text = self.get_text(endpoint, extra_query).await?;
        Ok(serde_json::from_str(&text).with_context(|| {
            format!(
                "Endpoint: {} ExtraQuery: {:?} JSON: {}",
                endpoint, extra_query, text,
            )
        })?)
    }

    /// Returns None when authentication has failed.
    pub async fn authenticate_user(&self) -> anyhow::Result<Option<UserId>> {
        let text = self.get_text("AuthenticateUser", &[]).await?;
        if text == "null" {
            trace!("Got a null response, failing authentication.");
            return Ok(None);
        }

        // If it's not "null" we expect a number!
        let id = text.parse::<u64>()?;
        trace!("Successful authentication: {}", id);
        Ok(Some(id.into()))
    }

    pub async fn get_games_and_players(
        &self,
        player_ids: &[&UserId],
    ) -> anyhow::Result<GetGamesAndPlayers> {
        let player_id_text = player_ids
            .iter()
            .map(|u| format!("{}", u))
            .collect::<Vec<_>>()
            .join("_");
        self.get_json("GetGamesAndPlayers", &[("playerIDText", &player_id_text)])
            .await
    }

    #[instrument(skip(self))]
    pub async fn get_latest_save_file_bytes(
        &self,
        game_id: &GameId,
        save_path: &PathBuf,
    ) -> anyhow::Result<(mpsc::Receiver<DownloadMessage>, JoinHandle<()>)> {
        trace!("Starting download.");
        let s = self.clone();
        let game_id = game_id.clone();
        let (tx, rx) = mpsc::channel(32);
        let save_path = save_path.clone();
        let handle = tokio::spawn(async move {
            let response = s
                .get(
                    "GetLatestSaveFileBytes",
                    &[("gameId", &format!("{}", game_id))],
                )
                .await
                .unwrap(); // TODO: unwrap
            let size = response.content_length();
            trace!(?size);
            tx.send(DownloadMessage::Started(size)).await.unwrap();

            let mut stream = response.bytes_stream();
            let mut temp_file = NamedTempFile::new().unwrap(); // TODO: unwrap
            let mut downloaded = 0;
            while let Some(bytes) = stream.next().await {
                let bytes = bytes.unwrap();
                downloaded += bytes.len();
                temp_file.write_all(&bytes).unwrap(); // TODO: lots of unwrap
                let percentage =
                    size.map(|size| (downloaded as f32 / size as f32).try_into().unwrap()); // TODO: unwrap
                tx.send(DownloadMessage::Chunk(percentage)).await.unwrap();
            }
            info!(?save_path, "Saving to disk.");
            temp_file.persist(&save_path).unwrap(); // TODO: unwrap
            tx.send(DownloadMessage::Done(save_path)).await.unwrap();
            trace!("Done.");
        });

        Ok((rx, handle))
    }

    #[instrument(skip(self))]
    pub async fn upload_save_client(
        &self,
        turn_id: TurnId,
        bytes: Vec<u8>,
    ) -> anyhow::Result<(mpsc::Receiver<UploadMessage>)> {
        let (tx, rx) = mpsc::channel(32);

        let s = self.clone();
        tokio::spawn(async move {
            trace!("Starting upload.");
            tx.send(UploadMessage::Started).await?;

            let auth_key = s.auth_key.clone();
            let form = Form::new()
                .part("turnId", text_part(format!("{}", turn_id)))
                .part("isCompressed", text_part("False".into()))
                .part("authKey", text_part(auth_key))
                .part(
                    "saveFileUpload",
                    Part::bytes(bytes).file_name(format!("{}.Civ5Save", turn_id)),
                );

            let url = "http://multiplayerrobot.com/Game/UploadSaveClient";
            let response = reqwest::Client::new()
                .post(url)
                .multipart(form)
                .send()
                .await?;
            trace!("Upload done.");

            let text = response.text().await?;
            let resp: UploadResponse = serde_json::from_str(&text)?;
            trace!(?resp);
            if resp.result_type == 0 {
                return Err(anyhow!("Response returned 0 for an unknown reason."));
            }

            tx.send(UploadMessage::Done).await?;
            Ok(())
        });

        Ok(rx)
    }
}

fn text_part(s: String) -> Part {
    Part::text(s).mime_str("text/plain; charset=utf-8").unwrap()
}
