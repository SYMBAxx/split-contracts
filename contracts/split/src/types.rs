use soroban_sdk::{contracttype, Address, Vec};

// ---------------------------------------------------------------------------
// Core invoice types
// ---------------------------------------------------------------------------

/// Status of an invoice lifecycle.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum InvoiceStatus {
    /// Invoice created, awaiting full payment.
    Pending,
    /// All shares paid; funds released to recipients.
    Released,
    /// Deadline passed before full funding; payers refunded.
    Refunded,
}

/// A single payment made toward an invoice.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Payment {
    /// Address of the payer.
    pub payer: Address,
    /// Amount paid in stroops (7 decimal places).
    pub amount: i128,
}

/// An on-chain invoice splitting payment among multiple recipients.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Invoice {
    /// Address that created the invoice.
    pub creator: Address,
    /// Ordered list of recipient addresses.
    pub recipients: Vec<Address>,
    /// Amounts owed to each recipient (parallel to `recipients`).
    pub amounts: Vec<i128>,
    /// USDC token contract address.
    pub token: Address,
    /// Unix timestamp after which unfunded invoices can be refunded.
    pub deadline: u64,
    /// Total amount collected so far.
    pub funded: i128,
    /// Current lifecycle status.
    pub status: InvoiceStatus,
    /// All payments made toward this invoice.
    pub payments: Vec<Payment>,
}

// ---------------------------------------------------------------------------
// #865 – Creator collateral system
// ---------------------------------------------------------------------------

/// Collateral deposited by a creator to back their invoices.
#[contracttype]
#[derive(Clone, Debug)]
pub struct CreatorCollateral {
    /// Address of the creator who deposited collateral.
    pub creator: Address,
    /// Token used for collateral (typically USDC).
    pub token: Address,
    /// Amount of collateral currently held.
    pub amount: i128,
    /// Amount of collateral that is currently locked (backing active invoices).
    pub locked: i128,
}

// ---------------------------------------------------------------------------
// #866 – Invoice rating-based discount tiers
// ---------------------------------------------------------------------------

/// Rating record for a creator based on completed invoices.
#[contracttype]
#[derive(Clone, Debug)]
pub struct CreatorRating {
    /// Address of the rated creator.
    pub creator: Address,
    /// Cumulative sum of all rating scores (1–5 per invoice).
    pub total_score: u32,
    /// Total number of ratings received.
    pub count: u32,
}

/// Discount tier applied when creating an invoice based on creator rating.
///
/// Average rating brackets → basis-point discount on the invoice total:
/// - avg ≥ 4.5  → Tier1 (500 bps = 5 %)
/// - avg ≥ 3.5  → Tier2 (300 bps = 3 %)
/// - avg ≥ 2.5  → Tier3 (100 bps = 1 %)
/// - avg < 2.5  → None  (0 %)
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum DiscountTier {
    None,
    Tier1,
    Tier2,
    Tier3,
}

// ---------------------------------------------------------------------------
// #867 – Smart invoice routing based on recipient performance
// ---------------------------------------------------------------------------

/// Tracks how well a recipient fulfils their obligations (on-time deliveries, etc.).
#[contracttype]
#[derive(Clone, Debug)]
pub struct RecipientPerformance {
    /// Address of the recipient.
    pub recipient: Address,
    /// Number of invoices successfully completed (funds received).
    pub completed: u32,
    /// Number of invoices where the recipient was flagged (e.g. late/disputed).
    pub flagged: u32,
    /// Cumulative performance score (higher = better; incremented on completion,
    /// decremented on flag).
    pub score: i32,
}

/// Routing preference used when selecting recipients for a new invoice.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum RoutingStrategy {
    /// Use recipients exactly as supplied (default behaviour).
    Default,
    /// Prefer recipients with the highest performance score.
    PerformanceBased,
    /// Exclude any recipient whose score is below the supplied threshold.
    ThresholdBased,
}

// ---------------------------------------------------------------------------
// #868 – Creator covenant system with penalties
// ---------------------------------------------------------------------------

/// A covenant (commitment) made by a creator when submitting an invoice.
///
/// If the creator violates the covenant (e.g. cancels funded invoice, misses
/// delivery) a penalty is applied and deducted from their collateral.
#[contracttype]
#[derive(Clone, Debug)]
pub struct CreatorCovenant {
    /// Invoice this covenant is attached to.
    pub invoice_id: u64,
    /// Creator who made the commitment.
    pub creator: Address,
    /// Penalty amount (in stroops) slashed from collateral on violation.
    pub penalty_amount: i128,
    /// Whether the covenant has been violated.
    pub violated: bool,
    /// Whether the covenant was fulfilled (invoice released on time).
    pub fulfilled: bool,
}
