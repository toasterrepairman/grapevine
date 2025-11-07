use serde::Deserialize;
use std::collections::HashMap;

pub const APP_ID: &str = "com.toasterrepair.Grapevine";
pub const GDELT_API_URL: &str = "https://api.gdeltproject.org/api/v2/doc/doc";

#[derive(Debug, Clone)]
pub struct FirehosePost {
    pub timestamp: String,
    pub did: String,
    pub rkey: String,
    pub text: String,
    pub embed: Option<PostEmbed>,
    pub facets: Option<Vec<PostFacet>>,
}

#[derive(Debug, Clone)]
pub enum PostEmbed {
    Images { count: usize, alt_texts: Vec<String> },
    External { uri: String, title: String, description: String },
    Video,
}

#[derive(Debug, Clone)]
pub struct PostFacet {
    pub start: usize,
    pub end: usize,
    pub facet_type: FacetType,
}

#[derive(Debug, Clone)]
pub enum FacetType {
    Mention(String), // DID
    Link(String),    // URL
    Tag(String),     // Hashtag
}

#[derive(Debug, Deserialize, Clone)]
pub struct GdeltArticle {
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub seendate: String,
    #[serde(default)]
    pub socialimage: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub sourcecountry: String,
}

#[derive(Debug, Deserialize)]
pub struct GdeltResponse {
    #[serde(default)]
    pub articles: Vec<GdeltArticle>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FrankfurterRates {
    #[serde(flatten)]
    pub rates: HashMap<String, f64>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FrankfurterLatestResponse {
    pub base: String,
    pub date: String,
    pub rates: FrankfurterRates,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FrankfurterHistoricalResponse {
    pub base: String,
    pub start_date: String,
    pub end_date: String,
    pub rates: HashMap<String, FrankfurterRates>,
}

#[derive(Debug, Clone)]
pub struct CurrencyInfo {
    pub code: String,
    pub rate_to_usd: f64,
    pub change_24h: Option<f64>,
    pub change_7d: Option<f64>,
    pub trend_data: Vec<f64>,
}
