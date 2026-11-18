pub struct Fee {
    pub amount: u64,
    cents: u8,
}

pub fn compute(amount: u64) -> Fee {
    Fee { amount, cents: 0 }
}
