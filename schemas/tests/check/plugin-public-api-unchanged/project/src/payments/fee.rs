pub struct Fee {
    pub amount: u64,
    cents: u8,
}


/// Computes the fee.
pub fn compute(total: u64) -> Fee {
    let cents = (total % 100) as u8;
    Fee { amount: total, cents }
}
