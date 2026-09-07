//! 预算守卫：累计 token/成本，超额自动中断（R6）。

use crate::types::Usage;

#[derive(Debug, Clone)]
pub struct BudgetGuard {
    total_input: u64,
    total_output: u64,
    total_cached: u64,
    total_cost: f64,
    token_limit: u64,
    cost_limit: f64,
}

impl BudgetGuard {
    pub fn new(token_limit: u64, cost_limit: f64) -> Self {
        Self {
            total_input: 0,
            total_output: 0,
            total_cached: 0,
            total_cost: 0.0,
            token_limit,
            cost_limit,
        }
    }

    pub fn record(&mut self, usage: &Usage, price_in: f64, price_out: f64) {
        self.total_input += usage.prompt_tokens;
        self.total_output += usage.completion_tokens;
        self.total_cached += usage.cached_tokens;
        self.total_cost += usage.cost(price_in, price_out);
    }

    pub fn is_over_budget(&self) -> bool {
        let total_tokens = self.total_input + self.total_output;
        (self.token_limit > 0 && total_tokens >= self.token_limit)
            || (self.cost_limit > 0.0 && self.total_cost >= self.cost_limit)
    }

    pub fn total_input(&self) -> u64 {
        self.total_input
    }

    pub fn total_output(&self) -> u64 {
        self.total_output
    }

    pub fn total_cost(&self) -> f64 {
        self.total_cost
    }

    pub fn summary(&self) -> String {
        if self.total_cached > 0 {
            format!(
                "input={}, output={}, total={}, cached={}, cost=${:.4}",
                self.total_input,
                self.total_output,
                self.total_input + self.total_output,
                self.total_cached,
                self.total_cost
            )
        } else {
            format!(
                "input={}, output={}, total={}, cost=${:.4}",
                self.total_input,
                self.total_output,
                self.total_input + self.total_output,
                self.total_cost
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_calculation() {
        let u = Usage {
            prompt_tokens: 1_000_000,
            completion_tokens: 500_000,
            cached_tokens: 0,
        };
        // price_in=0.15, price_out=0.60 per 1M
        assert!((u.cost(0.15, 0.60) - 0.45).abs() < 1e-9);
    }

    #[test]
    fn budget_tracking() {
        let mut guard = BudgetGuard::new(1000, 1.0);
        let u = Usage {
            prompt_tokens: 400,
            completion_tokens: 300,
            cached_tokens: 0,
        };
        guard.record(&u, 0.15, 0.60);
        assert!(!guard.is_over_budget());
        assert_eq!(guard.total_input(), 400);
        assert_eq!(guard.total_output(), 300);

        guard.record(&u, 0.15, 0.60);
        // total = 1400 > 1000
        assert!(guard.is_over_budget());
    }

    #[test]
    fn cost_limit() {
        let mut guard = BudgetGuard::new(0, 0.01);
        let u = Usage {
            prompt_tokens: 100_000,
            completion_tokens: 50_000,
            cached_tokens: 0,
        };
        guard.record(&u, 1.0, 5.0); // cost = 0.1 + 0.25 = 0.35 > 0.01
        assert!(guard.is_over_budget());
    }

    #[test]
    fn no_limit() {
        let mut guard = BudgetGuard::new(0, 0.0);
        let u = Usage {
            prompt_tokens: 999_999,
            completion_tokens: 999_999,
            cached_tokens: 0,
        };
        guard.record(&u, 999.0, 999.0);
        assert!(!guard.is_over_budget());
    }
}
