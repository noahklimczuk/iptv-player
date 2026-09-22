//! Choosing which metadata result is actually the title in your library (README §4.5).
//!
//! Pure: no HTTP, no keys. The client lives in `aurora-ingest`; this decides what to do
//! with what comes back, which is the part that can be wrong in ways nobody notices.
//!
//! The rule throughout is that a wrong match is worse than no match. A library where
//! half the posters are missing is obviously incomplete; one where *The Matrix* wears
//! *The Matrix Reloaded*'s poster looks finished and is wrong, and nothing prompts the
//! user to go fix it.

use serde::{Deserialize, Serialize};

use crate::title;

/// Below this, no match is reported at all.
pub const MIN_CONFIDENCE: f32 = 0.72;

/// Two candidates closer together than this, with nothing to separate them, is a
/// coin flip rather than a match.
const AMBIGUITY_MARGIN: f32 = 0.06;

/// Release years differ by region and by cut. One year apart is normal; four apart is a
/// different film.
const YEAR_TOLERANCE: i32 = 2;

/// What we know about the local title we are trying to identify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub title: String,
    pub year: Option<i32>,
}

impl Query {
    pub fn new(title: impl Into<String>, year: Option<i32>) -> Self {
        Self {
            title: title.into(),
            year,
        }
    }

    /// Build a query from a raw VOD stream name, reusing the scene-name cleaner.
    pub fn from_stream_name(raw: &str) -> Self {
        let clean = title::clean_movie_title(raw);
        Self {
            title: clean.title,
            year: clean.year,
        }
    }
}

/// One result from a metadata search, reduced to what matching needs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub id: i64,
    pub title: String,
    /// The title in its original language, which is how foreign films are often listed
    /// in a playlist even when the provider's name is the English one.
    pub original_title: Option<String>,
    pub year: Option<i32>,
    pub popularity: f32,
}

impl Candidate {
    pub fn new(id: i64, title: impl Into<String>, year: Option<i32>) -> Self {
        Self {
            id,
            title: title.into(),
            original_title: None,
            year,
            popularity: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scored<'a> {
    pub candidate: &'a Candidate,
    pub score: f32,
}

/// Normalized word set, via the same `match_key` the rest of the library matches on.
fn tokens(s: &str) -> Vec<String> {
    title::match_key(s)
        .split_whitespace()
        .map(|t| t.to_string())
        .collect()
}

/// Token-set F1 between two titles.
///
/// F1 rather than plain overlap because the two failure directions matter differently
/// and both matter. Recall alone would let *The Matrix Reloaded* match *The Matrix*
/// perfectly — every query word is present. Precision alone would let *The Matrix*
/// match *The Matrix Reloaded*. Penalising extra words on either side is what keeps a
/// sequel from wearing its predecessor's poster.
pub fn title_similarity(a: &str, b: &str) -> f32 {
    let (ta, tb) = (tokens(a), tokens(b));
    if ta.is_empty() || tb.is_empty() {
        return 0.0;
    }
    if ta == tb {
        return 1.0;
    }

    let shared = ta.iter().filter(|t| tb.contains(t)).count() as f32;
    if shared == 0.0 {
        return 0.0;
    }
    let precision = shared / tb.len() as f32;
    let recall = shared / ta.len() as f32;
    2.0 * precision * recall / (precision + recall)
}

/// How well one candidate answers the query, in `0.0..=1.0`.
pub fn score(query: &Query, candidate: &Candidate) -> f32 {
    let by_title = title_similarity(&query.title, &candidate.title);
    let by_original = candidate
        .original_title
        .as_deref()
        .map(|o| title_similarity(&query.title, o))
        .unwrap_or(0.0);
    let mut score = by_title.max(by_original);
    if score == 0.0 {
        return 0.0;
    }

    match (query.year, candidate.year) {
        (Some(want), Some(got)) => {
            let diff = (want - got).abs();
            if diff > YEAR_TOLERANCE {
                // The strongest negative signal there is. A title that matches word for
                // word but is four years out is a remake, a sequel, or a different film
                // with the same name — and we cannot tell which, so we decline.
                score *= 0.4;
            } else if diff == 0 {
                score = (score + 0.12).min(1.0);
            } else if diff == 1 {
                score = (score + 0.04).min(1.0);
            }
        }
        // The provider gave a year and the candidate has none: weak, but not wrong.
        (Some(_), None) => score *= 0.95,
        _ => {}
    }

    // Popularity breaks ties and nothing else. Letting it do more would mean a blockbuster
    // outranking the obscure film the user actually has.
    let tiebreak = (candidate.popularity.max(0.0) + 1.0).ln() * 0.004;
    (score + tiebreak.min(0.02)).min(1.0)
}

/// The best candidate, or `None` when nothing is confident enough or two results are
/// too close to separate.
pub fn pick_best<'a>(query: &Query, candidates: &'a [Candidate]) -> Option<Scored<'a>> {
    let mut scored: Vec<Scored<'a>> = candidates
        .iter()
        .map(|candidate| Scored {
            candidate,
            score: score(query, candidate),
        })
        .collect();
    // Descending, with the id as a tiebreak so the result never depends on input order.
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.candidate.id.cmp(&b.candidate.id))
    });

    let best = scored.first()?.clone();
    if best.score < MIN_CONFIDENCE {
        return None;
    }

    // Two near-identical scores mean we cannot tell them apart. With a year to go on
    // that is rare; without one, "Alone" could be any of six films, and picking the
    // popular one would be a guess wearing a confident face.
    if let Some(second) = scored.get(1) {
        if best.score - second.score < AMBIGUITY_MARGIN {
            return None;
        }
    }
    Some(best)
}

/// A person on a title's cast or crew list (README §5, `people` / `credits`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Person {
    pub tmdb_id: i64,
    pub name: String,
    pub profile_path: Option<String>,
}

/// One person's involvement in one title.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Credit {
    pub person: Person,
    /// Character name for cast, job title for crew.
    pub role: Option<String>,
    pub is_cast: bool,
    /// Billing position; lower is more prominent.
    pub order: u16,
}

/// Everything enrichment writes onto a title.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub tmdb_id: i64,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub logo_path: Option<String>,
    pub runtime_mins: Option<u32>,
    pub rating: Option<f32>,
    pub certification: Option<String>,
    pub genres: Vec<String>,
    pub credits: Vec<Credit>,
}

impl Metadata {
    /// The billed cast, most prominent first — what a detail page lists.
    pub fn cast(&self) -> Vec<&Credit> {
        let mut cast: Vec<&Credit> = self.credits.iter().filter(|c| c.is_cast).collect();
        cast.sort_by_key(|c| c.order);
        cast
    }

    /// Everyone credited with a given job, in billing order.
    pub fn crew_by_job(&self, job: &str) -> Vec<&Credit> {
        let mut crew: Vec<&Credit> = self
            .credits
            .iter()
            .filter(|c| {
                !c.is_cast
                    && c.role
                        .as_deref()
                        .is_some_and(|r| r.eq_ignore_ascii_case(job))
            })
            .collect();
        crew.sort_by_key(|c| c.order);
        crew
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_popularity(mut c: Candidate, popularity: f32) -> Candidate {
        c.popularity = popularity;
        c
    }

    #[test]
    fn identical_titles_score_perfectly() {
        assert_eq!(title_similarity("The Matrix", "The Matrix"), 1.0);
        // Normalization means punctuation and case do not count against it.
        assert_eq!(title_similarity("spider-man", "Spider Man"), 1.0);
    }

    #[test]
    fn a_sequel_does_not_look_like_its_predecessor() {
        let same = title_similarity("The Matrix", "The Matrix");
        let sequel = title_similarity("The Matrix", "The Matrix Reloaded");
        assert!(sequel < same, "a sequel must score below an exact match");
        // And symmetrically: neither direction is free.
        assert!(title_similarity("The Matrix Reloaded", "The Matrix") < same);
    }

    #[test]
    fn unrelated_titles_score_zero() {
        assert_eq!(title_similarity("Inception", "Amelie"), 0.0);
        assert_eq!(title_similarity("", "Inception"), 0.0);
        assert_eq!(title_similarity("Inception", ""), 0.0);
    }

    #[test]
    fn the_right_year_beats_the_wrong_one() {
        let query = Query::new("The Matrix", Some(1999));
        let right = score(&query, &Candidate::new(1, "The Matrix", Some(1999)));
        let wrong = score(&query, &Candidate::new(2, "The Matrix", Some(2015)));
        assert!(right > wrong);
        assert!(
            wrong < MIN_CONFIDENCE,
            "a four-year gap must not be accepted"
        );
    }

    #[test]
    fn a_year_one_off_is_still_a_match() {
        // A film released in December in the US and January elsewhere is the same film.
        let query = Query::new("Parasite", Some(2019));
        let scored = score(&query, &Candidate::new(1, "Parasite", Some(2020)));
        assert!(scored >= MIN_CONFIDENCE, "got {scored}");
    }

    #[test]
    fn the_original_title_can_carry_the_match() {
        let query = Query::new("Bienvenue chez les Ch'tis", None);
        let candidate = Candidate {
            original_title: Some("Bienvenue chez les Ch'tis".into()),
            ..Candidate::new(1, "Welcome to the Sticks", Some(2008))
        };
        assert!(score(&query, &candidate) >= MIN_CONFIDENCE);
    }

    #[test]
    fn picks_the_exact_match_out_of_a_result_list() {
        let query = Query::new("The Matrix", Some(1999));
        let candidates = vec![
            Candidate::new(2, "The Matrix Reloaded", Some(2003)),
            Candidate::new(1, "The Matrix", Some(1999)),
            Candidate::new(3, "The Matrix Revolutions", Some(2003)),
        ];
        let best = pick_best(&query, &candidates).expect("should match");
        assert_eq!(best.candidate.id, 1);
    }

    #[test]
    fn declines_rather_than_guessing_between_two_equals() {
        // Six films called "Alone" and no year to tell them apart. Picking one would be
        // a guess with a confident face.
        let query = Query::new("Alone", None);
        let candidates = vec![
            with_popularity(Candidate::new(1, "Alone", Some(2020)), 90.0),
            with_popularity(Candidate::new(2, "Alone", Some(2015)), 3.0),
        ];
        assert_eq!(pick_best(&query, &candidates), None);
    }

    #[test]
    fn a_year_resolves_what_popularity_cannot() {
        let query = Query::new("Alone", Some(2015));
        let candidates = vec![
            with_popularity(Candidate::new(1, "Alone", Some(2020)), 90.0),
            with_popularity(Candidate::new(2, "Alone", Some(2015)), 3.0),
        ];
        let best = pick_best(&query, &candidates).expect("the year separates them");
        assert_eq!(best.candidate.id, 2, "popularity must not outrank the year");
    }

    #[test]
    fn declines_when_nothing_is_close_enough() {
        let query = Query::new("Some Obscure Documentary", Some(2011));
        let candidates = [Candidate::new(1, "Jurassic Park", Some(1993))];
        assert_eq!(pick_best(&query, &candidates), None);
    }

    #[test]
    fn an_empty_result_list_is_not_a_match() {
        assert_eq!(pick_best(&Query::new("Anything", None), &[]), None);
    }

    #[test]
    fn popularity_only_breaks_a_tie_it_cannot_create_one() {
        // Same title, same year, different popularity: the popular one wins, but only
        // because there is genuinely nothing else to go on.
        let query = Query::new("Dune", Some(2021));
        let candidates = vec![
            with_popularity(Candidate::new(1, "Dune", Some(2021)), 1.0),
            with_popularity(Candidate::new(2, "Dune Drifter", Some(2020)), 500.0),
        ];
        let best = pick_best(&query, &candidates).expect("should match");
        assert_eq!(
            best.candidate.id, 1,
            "popularity must not beat a better title"
        );
    }

    #[test]
    fn the_result_does_not_depend_on_input_order() {
        let query = Query::new("Heat", Some(1995));
        let a = Candidate::new(7, "Heat", Some(1995));
        let b = Candidate::new(9, "Heat", Some(1995));
        let ab = [a.clone(), b.clone()];
        let ba = [b, a];
        let forward = pick_best(&query, &ab);
        let backward = pick_best(&query, &ba);
        // Both orders agree — here by both declining, since the two are identical.
        assert_eq!(forward, backward);
    }

    #[test]
    fn a_query_is_built_from_a_scene_style_stream_name() {
        let q = Query::from_stream_name("Inception.2010.1080p.WEB-DL.x265-MULTI");
        assert_eq!(q.title, "Inception");
        assert_eq!(q.year, Some(2010));

        let candidates = [Candidate::new(27205, "Inception", Some(2010))];
        let best = pick_best(&q, &candidates);
        assert_eq!(best.map(|s| s.candidate.id), Some(27205));
    }

    #[test]
    fn cast_comes_back_in_billing_order() {
        let person = |id: i64, name: &str| Person {
            tmdb_id: id,
            name: name.into(),
            profile_path: None,
        };
        let meta = Metadata {
            credits: vec![
                Credit {
                    person: person(3, "Third"),
                    role: Some("C".into()),
                    is_cast: true,
                    order: 2,
                },
                Credit {
                    person: person(1, "First"),
                    role: Some("A".into()),
                    is_cast: true,
                    order: 0,
                },
                Credit {
                    person: person(9, "Director"),
                    role: Some("Director".into()),
                    is_cast: false,
                    order: 0,
                },
                Credit {
                    person: person(2, "Second"),
                    role: Some("B".into()),
                    is_cast: true,
                    order: 1,
                },
            ],
            ..Default::default()
        };

        assert_eq!(
            meta.cast()
                .iter()
                .map(|c| c.person.name.as_str())
                .collect::<Vec<_>>(),
            vec!["First", "Second", "Third"],
            "crew must not appear in the cast list"
        );
        assert_eq!(
            meta.crew_by_job("director")
                .iter()
                .map(|c| c.person.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Director"],
            "job lookup is case-insensitive"
        );
        assert!(meta.crew_by_job("Composer").is_empty());
    }
}
