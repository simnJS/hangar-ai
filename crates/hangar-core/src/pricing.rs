//! What the tokens of the usage statistics would have cost at API list
//! prices — the « coût équivalent API » of the stats page.
//!
//! It is an equivalent, not a bill. A Claude Pro or Max subscriber pays a
//! flat fee, and the page says so next to every figure this module produces.
//! What it answers is "how much work did the agents do", in the one unit that
//! weighs an output token against a cached one the way Anthropic itself does.
//! Claude Code is the only agent Hangar runs (owner's decision, 2026-11-03),
//! so only Anthropic's models are priced.
//!
//! Prices are configuration, kept in one table rather than spread through the
//! code: they move with every model release, and this is the one place to
//! correct when they do. A model the table does not know costs nothing rather
//! than a guess — the page lists those apart, so a missing price shows as one.
//!
//! Sources, read on 2026-10-26:
//! - platform.claude.com/docs/en/about-claude/pricing (model pricing, fast
//!   mode, and long context pricing).

/// US dollars per million tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rates {
    pub input: f64,
    pub output: f64,
    /// Writing a prefix to the five-minute prompt cache.
    pub cache_write_5m: f64,
    /// Writing it to the one-hour cache, which Claude Code also uses.
    pub cache_write_1h: f64,
    /// Reading a prefix back from the cache.
    pub cache_read: f64,
}

/// Higher rates for a request whose prompt is over a threshold, counted the
/// way Anthropic does: every input token of the request, cached or not.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LongContext {
    pub over: u64,
    pub rates: Rates,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pricing {
    pub rates: Rates,
    pub long: Option<LongContext>,
    /// Multiplier for a request served in fast mode (`usage.speed`), which
    /// applies to every category, the cache ones included.
    pub fast: f64,
}

/// Anthropic's grid: writes at 1.25× and 2× the input rate, reads at the rate
/// the table gives — 0.1× on most models, less on the newest.
const fn claude(input: f64, output: f64, cache_read: f64) -> Rates {
    Rates {
        input,
        output,
        cache_write_5m: input * 1.25,
        cache_write_1h: input * 2.0,
        cache_read,
    }
}

const fn standard(rates: Rates) -> Pricing {
    Pricing {
        rates,
        long: None,
        fast: 1.0,
    }
}

/// Claude model id fragments, first match wins: the more specific id comes
/// before the one it extends (`opus-5-5` before `opus-5`).
const CLAUDE: &[(&str, Pricing)] = &[
    ("fable-5-1", standard(claude(10.0, 50.0, 0.25))),
    ("mythos-5-1", standard(claude(10.0, 50.0, 0.25))),
    ("fable-5", standard(claude(10.0, 50.0, 1.0))),
    ("mythos-5", standard(claude(10.0, 50.0, 1.0))),
    (
        "opus-5-5",
        Pricing {
            rates: claude(4.0, 20.0, 0.20),
            long: None,
            // $8 / $40 in fast mode.
            fast: 2.0,
        },
    ),
    (
        "opus-5",
        Pricing {
            rates: claude(5.0, 25.0, 0.50),
            long: None,
            fast: 2.0,
        },
    ),
    (
        "opus-4-8",
        Pricing {
            rates: claude(5.0, 25.0, 0.50),
            long: None,
            fast: 2.0,
        },
    ),
    ("opus-4-7", standard(claude(5.0, 25.0, 0.50))),
    ("opus-4-6", standard(claude(5.0, 25.0, 0.50))),
    ("opus-4-5", standard(claude(5.0, 25.0, 0.50))),
    ("opus-4-1", standard(claude(15.0, 75.0, 1.50))),
    ("opus-4", standard(claude(15.0, 75.0, 1.50))),
    ("sonnet-5-5", standard(claude(2.0, 10.0, 0.10))),
    ("sonnet-5", standard(claude(2.0, 10.0, 0.20))),
    ("sonnet-4", standard(claude(3.0, 15.0, 0.30))),
    ("3-7-sonnet", standard(claude(3.0, 15.0, 0.30))),
    ("3-5-sonnet", standard(claude(3.0, 15.0, 0.30))),
    (
        "haiku-5-5",
        Pricing {
            rates: claude(0.10, 0.50, 0.01),
            // Haiku 5.5 is priced by prompt length, at five times the rates
            // past 100,000 tokens.
            long: Some(LongContext {
                over: 100_000,
                rates: claude(0.50, 2.50, 0.05),
            }),
            fast: 1.0,
        },
    ),
    ("haiku-4-5", standard(claude(1.0, 5.0, 0.10))),
    ("3-5-haiku", standard(claude(0.80, 4.0, 0.08))),
];

/// The price grid of a model, or `None` when the table does not know it.
pub fn pricing(model: &str) -> Option<&'static Pricing> {
    // `claude-opus-5-5[1m]` names the same model with its long context on.
    let id = model.trim().to_ascii_lowercase();
    let id = id.split('[').next().unwrap_or_default();
    // A fragment is only looked for in a Claude id: another provider's model
    // that happens to contain one is not priced as Claude.
    if !id.contains("claude") && !CLAUDE.iter().any(|(name, _)| id == *name) {
        return None;
    }
    CLAUDE
        .iter()
        .find(|(fragment, _)| id.contains(fragment))
        .map(|(_, pricing)| pricing)
}

/// The tokens of one API call, split the way Anthropic bills them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CallTokens {
    /// Input that was neither read from nor written to the cache.
    pub input: u64,
    pub output: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub cache_read: u64,
    pub fast: bool,
}

/// What one call would have cost at list price, in dollars, by kind of token.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Cost {
    pub input: f64,
    pub output: f64,
    /// Both cache durations together, as the page shows them.
    pub cache_write: f64,
    pub cache_read: f64,
}

impl Cost {
    pub fn total(&self) -> f64 {
        self.input + self.output + self.cache_write + self.cache_read
    }
}

/// What one call would have cost at list price; `None` for a model without
/// a price.
///
/// This looks the model up first. A loop over many calls resolves the
/// `Pricing` once per model with `pricing()` and asks `Pricing::cost`.
pub fn call_cost(model: &str, tokens: &CallTokens) -> Option<Cost> {
    pricing(model).map(|pricing| pricing.cost(tokens))
}

impl Pricing {
    /// What one call would have cost at these prices.
    pub fn cost(&self, tokens: &CallTokens) -> Cost {
        let prompt =
            tokens.input + tokens.cache_write_5m + tokens.cache_write_1h + tokens.cache_read;
        let rates = match self.long {
            Some(long) if prompt > long.over => long.rates,
            _ => self.rates,
        };
        let speed = if tokens.fast { self.fast } else { 1.0 };
        let per = |count: u64, rate: f64| count as f64 * rate * speed / 1_000_000.0;
        Cost {
            input: per(tokens.input, rates.input),
            output: per(tokens.output, rates.output),
            cache_write: per(tokens.cache_write_5m, rates.cache_write_5m)
                + per(tokens.cache_write_1h, rates.cache_write_1h),
            cache_read: per(tokens.cache_read, rates.cache_read),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn claude_ids_find_their_own_row_before_a_shorter_one() {
        let rates = |id: &str| pricing(id).expect(id).rates;
        assert_eq!(rates("claude-opus-5-5").input, 4.0);
        assert_eq!(rates("claude-opus-5").input, 5.0);
        assert_eq!(rates("claude-opus-5-5[1m]").cache_read, 0.20);
        assert_eq!(rates("claude-fable-5-1").cache_read, 0.25);
        assert_eq!(rates("claude-fable-5").cache_read, 1.0);
        assert_eq!(rates("claude-haiku-4-5-20251001").output, 5.0);
        assert_eq!(rates("claude-opus-4-1-20250805").input, 15.0);
        assert_eq!(rates("claude-3-5-sonnet-20241022").input, 3.0);
        assert_eq!(rates("claude-sonnet-5-5").cache_read, 0.10);
        assert!(pricing("<synthetic>").is_none());
        assert!(pricing("claude-unknown-9").is_none());
    }

    #[test]
    fn a_model_of_another_provider_has_no_price() {
        assert!(pricing("gpt-5.5").is_none());
        // A Claude fragment inside another id does not make it Claude.
        assert!(pricing("my-sonnet-5-finetune").is_none());
    }

    #[test]
    fn every_category_is_billed_at_its_own_rate() {
        let tokens = CallTokens {
            input: 1_000_000,
            output: 1_000_000,
            cache_write_5m: 1_000_000,
            cache_write_1h: 1_000_000,
            cache_read: 1_000_000,
            fast: false,
        };
        // 4 + 20 + 5 + 8 + 0.20
        assert!(close(
            call_cost("claude-opus-5-5", &tokens).unwrap().total(),
            37.2
        ));
        let fast = CallTokens {
            fast: true,
            ..tokens
        };
        assert!(close(
            call_cost("claude-opus-5-5", &fast).unwrap().total(),
            74.4
        ));
        assert_eq!(call_cost("mystery-model", &tokens), None);
    }

    #[test]
    fn a_long_prompt_moves_the_whole_request_to_the_long_context_rates() {
        let short = CallTokens {
            input: 50_000,
            cache_read: 50_000,
            output: 1_000,
            ..CallTokens::default()
        };
        let long = CallTokens {
            cache_read: 50_001,
            ..short
        };
        // Haiku 5.5: 0.10 in, 0.01 cached, 0.50 out; five times that past 100K.
        let at_short = 0.05 * 0.10 + 0.05 * 0.01 + 0.001 * 0.50;
        assert!(close(
            call_cost("claude-haiku-5-5", &short).unwrap().total(),
            at_short
        ));
        let at_long = 0.05 * 0.50 + 0.050001 * 0.05 + 0.001 * 2.50;
        assert!(close(
            call_cost("claude-haiku-5-5", &long).unwrap().total(),
            at_long
        ));
    }
}
