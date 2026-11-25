use super::facts::{Facts, genre_name};
use serde_json::Value;
use std::collections::BTreeMap;

const WIKIPEDIA: &str = "https://en.wikipedia.org";
const WIKIDATA: &str = "https://www.wikidata.org/w/api.php";
const DEVELOPER: &str = "P178";
const PUBLISHER: &str = "P123";
const GENRE: &str = "P136";
const DATE: &str = "P577";

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Page {
    pub title: String,
    pub url: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Claims {
    developer: Option<String>,
    publisher: Option<String>,
    genre: Option<String>,
    date: Option<(u16, Option<u8>, Option<u8>)>,
}

pub fn search_url(name: &str) -> String {
    format!(
        "{WIKIPEDIA}/w/rest.php/v1/search/title?q={}&limit=5",
        crate::covers::encode(name)
    )
}

pub fn pick(body: &[u8]) -> Option<(String, Page)> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let page = value["pages"].as_array()?.iter().find(|page| {
        page["description"].as_str().is_some_and(|text| {
            let text = text.to_lowercase();
            text.contains("video game") && !text.contains("series") && !text.contains("franchise")
        })
    })?;
    let key = page["key"].as_str()?;
    let title = page["title"].as_str().unwrap_or(key);
    Some((
        key.to_owned(),
        Page {
            title: title.to_owned(),
            url: format!("{WIKIPEDIA}/wiki/{}", crate::covers::encode(key)),
        },
    ))
}

pub fn claims_url(key: &str) -> String {
    format!(
        "{WIKIDATA}?action=wbgetentities&sites=enwiki&titles={}&props=claims&format=json&origin=*",
        crate::covers::encode(key)
    )
}

pub fn claims(body: &[u8]) -> Option<Claims> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let claims = &value["entities"].as_object()?.values().next()?["claims"];
    let first = |property: &str| {
        claims[property][0]["mainsnak"]["datavalue"]["value"]["id"]
            .as_str()
            .map(str::to_owned)
    };
    let date = claims[DATE]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|claim| {
            let value = &claim["mainsnak"]["datavalue"]["value"];
            date_of(value["time"].as_str()?, value["precision"].as_u64()?)
        })
        .min();
    Some(Claims {
        developer: first(DEVELOPER),
        publisher: first(PUBLISHER),
        genre: first(GENRE),
        date,
    })
}

fn date_of(time: &str, precision: u64) -> Option<(u16, Option<u8>, Option<u8>)> {
    let mut parts = time.strip_prefix('+')?.split(['-', 'T']);
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok().filter(|month| *month > 0);
    let day = parts.next()?.parse().ok().filter(|day| *day > 0);
    Some((
        year,
        month.filter(|_| precision >= 10),
        day.filter(|_| precision >= 11),
    ))
}

impl Claims {
    pub fn ids(&self) -> Vec<&str> {
        [&self.developer, &self.publisher, &self.genre]
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect()
    }

    pub fn facts(&self, labels: &BTreeMap<String, String>) -> Facts {
        let label = |id: &Option<String>| id.as_ref().and_then(|id| labels.get(id)).cloned();
        Facts {
            developer: label(&self.developer),
            publisher: label(&self.publisher),
            genre: label(&self.genre).map(|genre| genre_name(&genre)),
            year: self.date.map(|date| date.0),
            month: self.date.and_then(|date| date.1),
            day: self.date.and_then(|date| date.2),
        }
    }
}

pub fn labels_url(ids: &[&str]) -> String {
    format!(
        "{WIKIDATA}?action=wbgetentities&ids={}&props=labels&languages=en&format=json&origin=*",
        ids.join("%7C")
    )
}

pub fn labels(body: &[u8]) -> BTreeMap<String, String> {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return BTreeMap::new();
    };
    value["entities"]
        .as_object()
        .into_iter()
        .flatten()
        .filter_map(|(id, entity)| {
            Some((
                id.clone(),
                entity["labels"]["en"]["value"].as_str()?.to_owned(),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH: &str = r#"{"pages":[
{"id":1,"key":"Castlevania","title":"Castlevania","excerpt":"","matched_title":null,"anchor":null,"description":"Video game series","thumbnail":null},
{"id":2,"key":"Castlevania:_Aria_of_Sorrow","title":"Castlevania: Aria of Sorrow","excerpt":"","matched_title":null,"anchor":null,"description":"2003 video game","thumbnail":null}]}"#;

    const CLAIMS: &str = r#"{"entities":{"Q2291309":{"claims":{
"P178":[{"mainsnak":{"datavalue":{"value":{"id":"Q1"}}}}],
"P123":[{"mainsnak":{"datavalue":{"value":{"id":"Q2"}}}}],
"P136":[{"mainsnak":{"datavalue":{"value":{"id":"Q3"}}}}],
"P577":[{"mainsnak":{"datavalue":{"value":{"time":"+2003-06-10T00:00:00Z","precision":11}}}},
        {"mainsnak":{"datavalue":{"value":{"time":"+2003-05-06T00:00:00Z","precision":11}}}}]}}}}"#;

    const LABELS: &str = r#"{"entities":{"Q1":{"labels":{"en":{"value":"Konami Computer Entertainment Tokyo"}}},
"Q2":{"labels":{"en":{"value":"Konami"}}},"Q3":{"labels":{"en":{"value":"action role-playing game"}}}}}"#;

    #[test]
    fn the_search_picks_the_game_and_skips_the_series() {
        let (key, page) = pick(SEARCH.as_bytes()).unwrap();
        assert_eq!(key, "Castlevania:_Aria_of_Sorrow");
        assert_eq!(page.title, "Castlevania: Aria of Sorrow");
        assert_eq!(
            page.url,
            "https://en.wikipedia.org/wiki/Castlevania%3A_Aria_of_Sorrow"
        );
        assert_eq!(pick(br#"{"pages":[]}"#), None);
    }

    #[test]
    fn wikidata_claims_become_facts_with_the_earliest_date() {
        let claims = claims(CLAIMS.as_bytes()).unwrap();
        assert_eq!(claims.ids(), ["Q1", "Q2", "Q3"]);
        let facts = claims.facts(&labels(LABELS.as_bytes()));
        assert_eq!(
            facts.developer.as_deref(),
            Some("Konami Computer Entertainment Tokyo")
        );
        assert_eq!(facts.publisher.as_deref(), Some("Konami"));
        assert_eq!(facts.genre.as_deref(), Some("Role-playing (RPG)"));
        assert_eq!(
            (facts.year, facts.month, facts.day),
            (Some(2003), Some(5), Some(6))
        );
    }

    #[test]
    fn a_page_wikidata_lacks_gives_no_facts() {
        let claims =
            claims(br#"{"entities":{"-1":{"site":"enwiki","title":"X","missing":""}}}"#).unwrap();
        assert!(claims.ids().is_empty());
        assert_eq!(claims.facts(&BTreeMap::new()), Facts::default());
    }

    #[test]
    fn a_year_only_date_keeps_no_month() {
        assert_eq!(
            date_of("+1998-00-00T00:00:00Z", 9),
            Some((1998, None, None))
        );
        assert_eq!(
            date_of("+1998-10-21T00:00:00Z", 10),
            Some((1998, Some(10), None))
        );
    }
}
