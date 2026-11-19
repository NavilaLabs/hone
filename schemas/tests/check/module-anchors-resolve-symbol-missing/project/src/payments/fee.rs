pub struct Fee {
    pub amount: u64,
    cents: u8,
}

impl Fee {
    pub fn total(&self) -> u64 {
        self.amount
    }
}

pub enum Tier {
    Low,
    High,
}

pub mod rounding {
    pub fn round(amount: u64) -> u64 {
        amount
    }
}

pub fn compute(amount: u64) -> Fee {
    Fee { amount, cents: 0 }
}
