#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Symbol};

// ----------------------------------------------------------------------
// 1. DATA STRUCTURES
// ----------------------------------------------------------------------

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum JobState {
    Funded = 0,     // Money is locked, work is active
    Completed = 1,  // Work done & Paid
    Cancelled = 2,  // Cancelled (Refunded)
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Job {
    pub client: Address,
    pub freelancer: Address, // DIRECT Address (No longer Option, since we agreed off-chain)
    pub token: Address,      // USDC Address
    pub amount: i128,        // The Agreed Price
    
    // DEADLINE & PENALTY LOGIC
    pub soft_deadline: u64,    // Full payout before this time
    pub hard_deadline: u64,    // Zero payout after this time
    pub penalty_per_sec: i128, // Deduction per second late
    
    pub state: JobState,
}

#[contracttype]
pub enum DataKey {
    Job(u64),       // Key: Job ID -> Value: Job Struct
    JobCounter,     // Key: "Counter" -> Value: Total jobs count
}

// ----------------------------------------------------------------------
// 2. CONTRACT LOGIC
// ----------------------------------------------------------------------

#[contract]
pub struct FreelanceContract;

#[contractimpl]
impl FreelanceContract {

    // STEP 1: CREATE ESCROW (Lock Money + Set Final Terms)
    // ----------------------------------------------------------------
    // This is called AFTER off-chain negotiation is finished.
    // It creates the job record AND pulls the money in one transaction.
    pub fn create_escrow(
        env: Env,
        client: Address,
        freelancer: Address,
        token: Address,
        amount: i128,
        soft_deadline: u64,
        hard_deadline: u64,
        penalty_per_sec: i128,
    ) -> u64 {
        // A. Security: Client signs to spend money
        client.require_auth();

        // B. Logic Checks
        if hard_deadline <= soft_deadline {
            panic!("Hard deadline must be after Soft deadline");
        }
        if amount <= 0 {
            panic!("Amount must be positive");
        }

        // C. TRANSFER FUNDS (Client -> Contract)
        // We do this IMMEDIATELY because the agreement is already done.
        let token_client = token::Client::new(&env, &token);
        token_client.transfer(
            &client,
            &env.current_contract_address(),
            &amount,
        );

        // D. Generate ID
        let mut count: u64 = env.storage().instance().get(&DataKey::JobCounter).unwrap_or(0);
        count += 1;

        // E. Create Job Record
        let new_job = Job {
            client,
            freelancer,
            token,
            amount,
            soft_deadline,
            hard_deadline,
            penalty_per_sec,
            state: JobState::Funded, // Starts directly as Funded
        };

        // F. Save & Rent
        env.storage().instance().set(&DataKey::JobCounter, &count);
        env.storage().persistent().set(&DataKey::Job(count), &new_job);
        env.storage().persistent().extend_ttl(&DataKey::Job(count), 17280, 34560);

        return count;
    }

    // STEP 2: COMPLETE JOB (Calculate Payout based on Time)
    // ----------------------------------------------------------------
    // This handles the math for the deadline penalties.
    pub fn complete_job(env: Env, job_id: u64) {
        // A. Load Job
        let mut job: Job = env.storage().persistent().get(&DataKey::Job(job_id)).expect("Job not found");

        // B. Security: Client approves the work
        // (In a real app, you might want the Freelancer to trigger this if using an Arbiter)
        job.client.require_auth();

        if job.state != JobState::Funded {
            panic!("Job is not active");
        }

        // C. CALCULATE PAYOUT
        let current_time = env.ledger().timestamp();
        let mut payout = job.amount;
        let mut refund = 0;

        // Scenario 1: On Time
        if current_time <= job.soft_deadline {
            payout = job.amount;
        } 
        // Scenario 2: Late (Between Soft and Hard)
        else if current_time < job.hard_deadline {
            let seconds_late = (current_time - job.soft_deadline) as i128;
            let penalty = seconds_late * job.penalty_per_sec;
            
            if penalty >= job.amount {
                payout = 0;
            } else {
                payout = job.amount - penalty;
            }
        } 
        // Scenario 3: Too Late (After Hard Deadline)
        else {
            payout = 0;
        }

        // Calculate Refund (Money saved from penalties goes back to Client)
        refund = job.amount - payout;

        // D. EXECUTE TRANSFERS
        let token_client = token::Client::new(&env, &job.token);

        // Pay Freelancer
        if payout > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &job.freelancer,
                &payout,
            );
        }

        // Refund Client
        if refund > 0 {
            token_client.transfer(
                &env.current_contract_address(),
                &job.client,
                &refund,
            );
        }

        // E. Close Job
        job.state = JobState::Completed;
        env.storage().persistent().set(&DataKey::Job(job_id), &job);
    }
}