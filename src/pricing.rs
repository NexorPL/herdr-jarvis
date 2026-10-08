use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Token counts for one or more assistant responses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
}

impl Usage {
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write_5m + self.cache_write_1h
    }
}

impl std::ops::AddAssign for Usage {
    fn add_assign(&mut self, o: Usage) {
        self.input += o.input;
        self.output += o.output;
        self.cache_read += o.cache_read;
        self.cache_write_5m += o.cache_write_5m;
        self.cache_write_1h += o.cache_write_1h;
    }
}

impl std::ops::SubAssign for Usage {
    fn sub_assign(&mut self, o: Usage) {
        self.input = self.input.saturating_sub(o.input);
        self.output = self.output.saturating_sub(o.output);
        self.cache_read = self.cache_read.saturating_sub(o.cache_read);
        self.cache_write_5m = self.cache_write_5m.saturating_sub(o.cache_write_5m);
        self.cache_write_1h = self.cache_write_1h.saturating_sub(o.cache_write_1h);
    }
}

/// USD per million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write_5m: f64,
    pub cache_write_1h: f64,
}

impl Price {
    /// Cache writes cost 1.25x input for the 5-minute TTL and 2x input for the 1-hour TTL.
    const fn standard(input: f64, output: f64, cache_read: f64) -> Price {
        Price {
            input,
            output,
            cache_read,
            cache_write_5m: input * 1.25,
            cache_write_1h: input * 2.0,
        }
    }
}

/// Anthropic first-party rates (cached 2026-06-24); the longest matching model-id prefix wins.
const DEFAULTS: &[(&str, Price)] = &[
    ("claude-fable-5-1", Price::standard(10.0, 50.0, 0.25)),
    ("claude-fable-5", Price::standard(10.0, 50.0, 1.0)),
    ("claude-mythos-5", Price::standard(10.0, 50.0, 1.0)),
    ("claude-opus-5-5", Price::standard(4.0, 20.0, 0.20)),
    ("claude-opus-5", Price::standard(5.0, 25.0, 0.50)),
    ("claude-opus-4-8", Price::standard(5.0, 25.0, 0.50)),
    ("claude-opus-4-7", Price::standard(5.0, 25.0, 0.50)),
    ("claude-opus-4-6", Price::standard(5.0, 25.0, 0.50)),
    ("claude-opus-4-5", Price::standard(5.0, 25.0, 0.50)),
    ("claude-opus-4", Price::standard(15.0, 75.0, 1.50)),
    ("claude-sonnet-5", Price::standard(2.0, 10.0, 0.20)),
    ("claude-sonnet-4", Price::standard(3.0, 15.0, 0.30)),
    ("claude-haiku-4-5", Price::standard(1.0, 5.0, 0.10)),
];

pub struct Pricing {
    overrides: HashMap<String, Price>,
}

impl Pricing {
    pub fn new(overrides: HashMap<String, Price>) -> Pricing {
        Pricing { overrides }
    }

    pub fn price(&self, model: &str) -> Option<Price> {
        if let Some(p) = self.overrides.get(model) {
            return Some(*p);
        }
        DEFAULTS
            .iter()
            .filter(|(prefix, _)| model.starts_with(prefix))
            .max_by_key(|(prefix, _)| prefix.len())
            .map(|(_, p)| *p)
    }

    /// Estimated cost in USD; `None` when the model has no known price.
    pub fn cost(&self, model: &str, u: &Usage) -> Option<f64> {
        let p = self.price(model)?;
        let micro = u.input as f64 * p.input
            + u.output as f64 * p.output
            + u.cache_read as f64 * p.cache_read
            + u.cache_write_5m as f64 * p.cache_write_5m
            + u.cache_write_1h as f64 * p.cache_write_1h;
        Some(micro / 1_000_000.0)
    }
}

/// A summed cost; `partial` is set when some part had no known price.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Cost {
    pub usd: f64,
    pub partial: bool,
}

impl Cost {
    pub fn add(&mut self, cost: Option<f64>) {
        match cost {
            Some(v) => self.usd += v,
            None => self.partial = true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn longest_prefix_wins() {
        let p = Pricing::new(HashMap::new());
        assert!(close(p.price("claude-opus-4-8").unwrap().input, 5.0));
        assert!(close(p.price("claude-opus-4-1").unwrap().input, 15.0));
        assert!(close(p.price("claude-opus-5-5").unwrap().input, 4.0));
        assert!(close(p.price("claude-opus-5").unwrap().input, 5.0));
    }

    #[test]
    fn cost_sums_all_token_kinds() {
        let p = Pricing::new(HashMap::new());
        let u = Usage {
            input: 10,
            output: 100,
            cache_read: 1000,
            cache_write_5m: 0,
            cache_write_1h: 2000,
        };
        // opus 5.5: 4 in, 20 out, 0.20 read, 8 per 1h write -> 40 + 2000 + 200 + 16000 per million
        assert!(close(p.cost("claude-opus-5-5", &u).unwrap(), 0.01824));
    }

    #[test]
    fn unknown_model_has_no_price() {
        let p = Pricing::new(HashMap::new());
        assert_eq!(p.cost("<synthetic>", &Usage::default()), None);
    }

    #[test]
    fn override_beats_default() {
        let custom = Price {
            input: 1.0,
            output: 1.0,
            cache_read: 1.0,
            cache_write_5m: 1.0,
            cache_write_1h: 1.0,
        };
        let p = Pricing::new(HashMap::from([("claude-opus-5-5".to_string(), custom)]));
        assert_eq!(p.price("claude-opus-5-5"), Some(custom));
    }

    #[test]
    fn cost_marks_partial_sums() {
        let mut c = Cost::default();
        c.add(Some(1.5));
        c.add(None);
        assert!(close(c.usd, 1.5));
        assert!(c.partial);
    }
}
