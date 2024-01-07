use crate::SteamID;
use crate::response::deserializers;
use serde::{Serialize, Deserialize};
use url::Url;
use std::borrow::Cow;

/// A ban.
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Ban {
    // todo fill this out
    // you probably won't see this appear often in responses for listings
}

/// A user.
#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: SteamID,
    pub name: String,
    pub avatar: String,
    pub avatar_full: String,
    #[serde(default)]
    #[serde(deserialize_with = "deserializers::default_on_null")]
    pub premium: bool,
    #[serde(default)]
    #[serde(deserialize_with = "deserializers::default_on_null")]
    pub online: bool,
    #[serde(default)]
    #[serde(deserialize_with = "deserializers::default_on_null")]
    pub banned: bool,
    pub custom_name_style: String,
    pub accepted_suggestions: u32,
    pub class: String,
    pub style: String,
    pub trade_offer_url: Option<String>,
    #[serde(default)]
    #[serde(deserialize_with = "deserializers::default_on_null")]
    pub is_marketplace_seller: bool,
    #[serde(default)]
    #[serde(deserialize_with = "deserializers::default_on_null")]
    pub flag_impersonated: bool,
    #[serde(default)]
    #[serde(deserialize_with = "deserializers::deserialize_listing_bans")]
    pub bans: Vec<Ban>,
}

impl User {
    pub fn access_token(&self) -> Option<String> {
        let trade_offer_url = self.trade_offer_url.as_ref()?;
        let url = Url::parse(trade_offer_url).ok()?;
            
        for (key, value) in url.query_pairs() {
            if key == Cow::Borrowed("token") {
                if value.len() == 8 {
                    return Some(value.to_string());
                } else {
                    // not a valid token
                    return None;
                }
            }
        }
        
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_listing() {
        let response: User = serde_json::from_str(include_str!("fixtures/user.json")).unwrap();
        let token = response.access_token();
        
        assert_eq!(token, Some("iF6QGWOa".into()));
    }
}

