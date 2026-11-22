//! The fee of a payment.

/// A fee in cents.
pub struct Fee {
    cents: u64,
}

impl Fee {
    /// A fee of `cents`.
    pub fn new(cents: u64) -> Self {
        Self { cents }
    }

    /// The sum of two fees.
    pub fn add(&self, other: &Fee) -> Fee {
        Fee::new(self.cents + other.cents)
    }

    /// The fee in cents.
    pub fn cents(&self) -> u64 {
        self.cents
    }
}
