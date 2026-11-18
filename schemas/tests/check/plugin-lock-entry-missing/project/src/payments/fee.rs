pub struct Fee {
    pub amount: u64,
    cents: u8,
}

/// Computes the fee.
pub fn compute(amount: u64) -> Fee {
    let cents = (amount % 100) as u8;
    Fee { amount, cents }
}
