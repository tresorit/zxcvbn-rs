//! `zxcvbn` is a password strength estimator based off of Dropbox's zxcvbn library.
//!
//! Through pattern matching and conservative estimation, it recognizes and weighs
//! 30k common passwords, common names and surnames according to US census data,
//! popular English words from Wikipedia and US television and movies, and other
//! common patterns like dates, repeats (aaa), sequences (abcd),
//! keyboard patterns (qwertyuiop), and l33t speak.
//!
//! Consider using zxcvbn as an algorithmic alternative to password composition policy —
//! it is more secure, flexible, and usable when sites require
//! a minimal complexity score in place of annoying rules like
//! "passwords must contain three of {lower, upper, numbers, symbols}".
#![cfg_attr(feature = "clippy", feature(plugin))]
#![cfg_attr(feature = "clippy", plugin(clippy))]
#![recursion_limit = "128"]
#![warn(missing_docs)]

#[macro_use]
extern crate derive_builder;
extern crate fancy_regex;
extern crate itertools;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate quick_error;
extern crate regex;
#[cfg(feature = "ser")]
extern crate serde;
#[cfg(feature = "ser")]
#[macro_use]
extern crate serde_derive;
#[cfg(feature = "time")]
extern crate time;

#[cfg(test)]
#[macro_use]
extern crate quickcheck;
#[cfg(test)]
extern crate serde_json;

pub use matching::Match;

mod adjacency_graphs;
pub mod feedback;
mod frequency_lists;
/// Defines structures for matches found in a password
pub mod matching;
mod scoring;
pub mod time_estimates;

/// Contains the results of an entropy calculation
#[derive(Debug, Clone)]
#[cfg_attr(feature = "ser", derive(Serialize))]
pub struct Entropy {
    /// Estimated guesses needed to crack the password
    pub guesses: u64,
    /// Order of magnitude of `guesses`
    pub guesses_log10: f64,
    /// List of back-of-the-envelope crack time estimations, in seconds, based on a few scenarios
    pub crack_times_seconds: time_estimates::CrackTimes,
    /// Same keys as `crack_time_seconds`, with human-readable display values,
    /// e.g. "less than a second", "3 hours", "centuries", etc.
    pub crack_times_display: time_estimates::CrackTimesDisplay,
    /// Overall strength score from 0-4.
    /// Any score less than 3 should be considered too weak.
    pub score: u8,
    /// Verbal feedback to help choose better passwords. Set when `score` <= 2.
    pub feedback: Option<feedback::Feedback>,
    /// The list of patterns the guess calculation was based on
    pub sequence: Vec<Match>,
    /// How long it took to calculate the answer, in milliseconds
    pub calc_time: u64,
}

quick_error! {
    #[derive(Debug, Clone, Copy)]
    /// Potential errors that may be returned from `zxcvbn`
    pub enum ZxcvbnError {
        /// Indicates that a blank password was passed in to `zxcvbn`
        BlankPassword {
            description("Zxcvbn cannot evaluate a blank password")
        }
    }
}

#[cfg(feature = "time")]
fn precise_time_ns() -> u64 {
    time::precise_time_ns()
}

#[cfg(not(feature = "time"))]
fn precise_time_ns() -> u64 {
    0
}

/// Takes a password string and optionally a list of user-supplied inputs
/// (e.g. username, email, first name) and calculates the strength of the password
/// based on entropy, using a number of different factors.
///
/// Currently zxcvbn only supports ASCII input. Non-ASCII passwords can generally be considered
/// to be safe, if they are of a reasonable length (8+ chars), so you should handle them as
/// strong passwords, but this library is not able to generate entropy information for them
/// at this time.
pub fn zxcvbn(password: &str, user_inputs: &[&str]) -> Result<Entropy, ZxcvbnError> {
    if password.is_empty() {
        return Err(ZxcvbnError::BlankPassword);
    }

    let start_time_ns = precise_time_ns();

    // Only evaluate the first 100 characters of the input.
    // This prevents potential DoS attacks from sending extremely long input strings.
    let password = password.chars().take(100).collect::<String>();

    let sanitized_inputs = user_inputs
        .iter()
        .enumerate()
        .map(|(i, x)| (x.to_lowercase(), i + 1))
        .collect();

    let matches = matching::omnimatch(&password, &sanitized_inputs);
    let result = scoring::most_guessable_match_sequence(&password, &matches, false);
    let calc_time = (precise_time_ns() - start_time_ns) / 1_000_000;
    let (attack_times, attack_times_display, score) =
        time_estimates::estimate_attack_times(result.guesses);
    let feedback = feedback::get_feedback(score, &matches);

    Ok(Entropy {
        guesses: result.guesses,
        guesses_log10: result.guesses_log10,
        crack_times_seconds: attack_times,
        crack_times_display: attack_times_display,
        score: score,
        feedback: feedback,
        sequence: result.sequence,
        calc_time: calc_time,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::TestResult;

    quickcheck! {
        fn test_zxcvbn_doesnt_panic(password: String, user_inputs: Vec<String>) -> TestResult {
            let inputs = user_inputs.iter().map(|s| s.as_ref()).collect::<Vec<&str>>();
            zxcvbn(&password, &inputs).ok();
            TestResult::from_bool(true)
        }

        #[cfg(feature = "ser")]
        fn test_zxcvbn_serialisation_doesnt_panic(password: String, user_inputs: Vec<String>) -> TestResult {
            let inputs = user_inputs.iter().map(|s| s.as_ref()).collect::<Vec<&str>>();
            serde_json::to_string(&zxcvbn(&password, &inputs).ok()).ok();
            TestResult::from_bool(true)
        }
    }

    #[test]
    fn test_zxcvbn() {
        let password = "r0sebudmaelstrom11/20/91aaaa";
        let entropy = zxcvbn(password, &[]).unwrap();
        assert_eq!(entropy.guesses, 473_471_216_704_000);
        assert_eq!(entropy.guesses_log10 as u16, 14);
        assert_eq!(entropy.score, 4);
        assert!(!entropy.sequence.is_empty());
        assert!(entropy.feedback.is_none());
    }

    #[test]
    fn test_zxcvbn_unicode() {
        let password = "𐰊𐰂𐰄𐰀𐰁";
        let entropy = zxcvbn(password, &[]).unwrap();
        assert_eq!(entropy.score, 1);
    }

    #[test]
    fn test_zxcvbn_unicode_2() {
        let password = "r0sebudmaelstrom丂/20/91aaaa";
        let entropy = zxcvbn(password, &[]).unwrap();
        assert_eq!(entropy.score, 4);
    }

    #[test]
    fn test_issue_13() {
        let password = "Imaginative-Say-Shoulder-Dish-0";
        let entropy = zxcvbn(password, &[]).unwrap();
        assert_eq!(entropy.score, 4);
    }

    #[test]
    fn test_issue_15_example_1() {
        let password = "TestMeNow!";
        let entropy = zxcvbn(password, &[]).unwrap();
        assert_eq!(entropy.guesses, 372_010_000);
        assert_eq!(entropy.guesses_log10, 8.57055461430783);
        assert_eq!(entropy.score, 3);
    }

    #[test]
    fn test_issue_15_example_2() {
        let password = "hey<123";
        let entropy = zxcvbn(password, &[]).unwrap();
        assert_eq!(entropy.guesses, 1_010_000);
        assert_eq!(entropy.guesses_log10, 6.004321373782642);
        assert_eq!(entropy.score, 2);
    }
}
