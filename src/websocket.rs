use crate::response::listing::Listing;
use crate::time::ServerTime;
use tokio_tungstenite::{tungstenite, connect_async};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use tokio::sync::mpsc;
use http::uri::Uri;
use http::request::Request;

const APPID_TEAM_FORTRESS_2: u32 = 440;

#[derive(Deserialize, Debug)]
enum EventType {
    #[serde(rename = "listing-update")]
    ListingUpdate,
    #[serde(rename = "listing-delete")]
    ListingDelete,
}

#[derive(Deserialize, Debug)]
struct EventMessage<'a> {
    id: &'a str,
    event: EventType,
    #[serde(borrow)]
    payload: &'a RawValue,
}

#[derive(Debug)]
pub enum Message {
    ListingUpdate(Listing),
    ListingDelete(Listing),
    ListingUpdateOtherApp {
        appid: u32,
        payload: Box<RawValue>,
    },
    ListingDeleteOtherApp {
        appid: u32,
        payload: Box<RawValue>,
    },
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{}", .0)]
    Url(#[from] http::uri::InvalidUri),
    #[error("Parsed URL does not contain hostname")]
    UrlNoHostName,
    #[error("Error parsing request parameters: {}", .0)]
    RequestParse(#[from] http::Error),
    #[error("{}", .0)]
    Connect(#[from] tungstenite::Error),
}

#[derive(Debug, Deserialize, Serialize)]
struct AppType {
    appid: u32,
}

/// Generate a random key for the `Sec-WebSocket-Key` header.
pub fn generate_key() -> String {
    // a base64-encoded (see Section 4 of [RFC4648]) value that,
    // when decoded, is 16 bytes in length (RFC 6455)
    let r: [u8; 16] = rand::random();
    data_encoding::BASE64.encode(&r)
}

pub async fn connect() -> Result<mpsc::Receiver<(ServerTime, Message)>, Error> {
    let connect_addr = "wss://ws.backpack.tf/events";
    let uri = connect_addr.parse::<Uri>()?;
    let authority = uri.authority()
        .ok_or(Error::UrlNoHostName)?.as_str();
    let host = authority
        .find('@')
        .map(|idx| authority.split_at(idx + 1).1)
        .unwrap_or_else(|| authority);
    let request = Request::builder()
        .header("batch-test", "true")
        .header("Host", host)
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", generate_key())
        .uri(uri)
        .body(())?;
    let (ws_stream, _) = connect_async(request).await?;
    let (write, read) = mpsc::channel::<(ServerTime, Message)>(100);
    let (_ws_write, mut ws_read) = ws_stream.split();

    tokio::spawn(async move {
        while let Some(message) = ws_read.next().await {
            match message {
                Ok(message) => {
                    let data = message.into_data();
                    let bytes = data.as_slice();

                    match serde_json::from_slice::<Vec<EventMessage>>(bytes) {
                        Ok(messages) => {
                            for message in messages {
                                if let Err(error) = on_event(&message, &write).await {
                                    match error {
                                        EventError::Serde(error) => {
                                            log::debug!(
                                                "Error deserializing event payload: {} {}",
                                                error,
                                                message.payload,
                                            );
                                        },
                                        EventError::HexDecode(error) => {
                                            log::debug!(
                                                "Error hex decoding event id: {} {}",
                                                error,
                                                message.id
                                            );
                                        },
                                        // connection likely dropped
                                        EventError::Send(_) => {
                                            break;
                                        },
                                    }
                                }
                            }
                        },
                        Err(error) => if bytes.is_empty() {
                            // the message is empty...
                            continue;
                        } else if let Ok(message) = std::str::from_utf8(bytes) {
                            log::debug!(
                                "Error deserializing event: {} {}",
                                error,
                                message,
                            );
                        } else {
                            log::debug!(
                                "Error deserializing event: {}; Invalid utf8 string: {:?}",
                                error,
                                bytes,
                            );
                        },
                    }
                },
                Err(error) => {
                    // dropped?
                    log::debug!(
                        "Connection dropped: {:?}",
                        error,
                    );
                    break;
                },
            }
        }

        drop(write);
    });

    Ok(read)
}

#[derive(thiserror::Error, Debug)]
enum EventError {
    #[error("{}", .0)]
    Send(#[from] tokio::sync::mpsc::error::SendError<(ServerTime, Message)>),
    #[error("{}", .0)]
    Serde(#[from] serde_json::Error),
    #[error("{}", .0)]
    HexDecode(#[from] data_encoding::DecodeError),
}

async fn on_event<'a>(
    message: &EventMessage<'a>,
    write: &mpsc::Sender<(ServerTime, Message)>,
) -> Result<(), EventError> {
    let event_time: ServerTime = ServerTime::from_timestamp(
        u32::from_be_bytes(
            data_encoding::HEXLOWER.decode(
                message.id.as_bytes()
            )?
            [..4].try_into().unwrap()
        ).into(),
        0
    ).unwrap();

    match message.event {
        EventType::ListingUpdate |
        EventType::ListingDelete => {
            match serde_json::from_str::<Listing>(message.payload.get()) {
                Ok(listing) => {
                    write.send(
                        (
                            event_time,
                            match message.event {
                                EventType::ListingUpdate => Message::ListingUpdate(listing),
                                EventType::ListingDelete => Message::ListingDelete(listing),
                            },
                        )
                    ).await?;
                },
                Err(error) => if let Ok(AppType { appid }) = serde_json::from_str::<AppType>(message.payload.get()) {
                    if appid != APPID_TEAM_FORTRESS_2 {
                        let payload = message.payload.to_owned();

                        write.send(
                            (
                                event_time,
                                match message.event {
                                    EventType::ListingUpdate => Message::ListingUpdateOtherApp {
                                        appid,
                                        payload,
                                    },
                                    EventType::ListingDelete => Message::ListingDeleteOtherApp {
                                        appid,
                                        payload,
                                    },
                                }
                            )
                        ).await?;
                    } else {
                        return Err(error.into());
                    }
                } else {
                    return Err(error.into());
                },
            }

            Ok(())
        },
    }
}
