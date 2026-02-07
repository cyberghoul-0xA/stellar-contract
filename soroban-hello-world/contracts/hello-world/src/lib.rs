#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String, symbol_short, Symbol};

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum JobState {
    Open = 0,     // 1. Posted by Client, open for applications
    Assigned = 1, // 2. Client selected a Freelancer & finalized terms
    Accepted = 2, // 3. Freelancer agreed to terms (Ready for funding)
    Funded = 3,   // 4. Money locked (Next Step)
    Completed = 4,// 5. Work done & Paid
    Failed = 5,   // 6. Hard deadline missed / Cancelled
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Job {
    pub client: Address,
    pub freelancer: Option<Address>, // Initially None
    pub token: Address,              // USDC Address
    pub amount: i128,                // Negotiated Amount
    
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
// 2. CONTRACT LOGIC (The Engine)
// ----------------------------------------------------------------------

#[contract]
pub struct FreelanceContract;

#[contractimpl]
impl FreelanceContract {

    // STEP 1: POST JOB (Client creates the initial offer)
    // ----------------------------------------------------------------
    pub fn post_job(
        env: Env,
        client: Address,
        token: Address,
        initial_amount: i128,
        soft_deadline: u64,
        hard_deadline: u64,
        penalty_per_sec: i128,
    ) -> u64 {
        // A. Security
        client.require_auth();

        // B. Logic Checks
        if hard_deadline <= soft_deadline {
            panic!("Hard deadline must be after Soft deadline");
        }
        if initial_amount <= 0 {
            panic!("Amount must be positive");
        }

        // C. Generate ID
        let mut count: u64 = env.storage().instance().get(&DataKey::JobCounter).unwrap_or(0);
        count += 1;

        // D. Create Job
        let new_job = Job {
            client,
            freelancer: None, // No freelancer yet
            token,
            amount: initial_amount,
            soft_deadline,
            hard_deadline,
            penalty_per_sec,
            state: JobState::Open,
        };

        // E. Save & Rent
        env.storage().instance().set(&DataKey::JobCounter, &count);
        env.storage().persistent().set(&DataKey::Job(count), &new_job);
        env.storage().persistent().extend_ttl(&DataKey::Job(count), 17280, 34560);

        return count;
    }

    // STEP 2: ASSIGN & NEGOTIATE (Client selects Freelancer with final terms)
    // ----------------------------------------------------------------
    pub fn assign_freelancer(
        env: Env,
        job_id: u64,
        freelancer: Address,
        final_amount: i128,
        final_soft: u64,
        final_hard: u64,
        final_penalty: i128
    ) {
        // A. Load Job
        let mut job: Job = env.storage().persistent().get(&DataKey::Job(job_id)).expect("Job not found");

        // B. Security: Only Client can assign
        job.client.require_auth();

        // C. Logic: Must be Open
        if job.state != JobState::Open {
            panic!("Job is not Open");
        }
        if final_hard <= final_soft {
            panic!("Hard deadline must be after Soft deadline");
        }

        // D. Update Job with Final Terms
        job.freelancer = Some(freelancer);
        job.amount = final_amount;
        job.soft_deadline = final_soft;
        job.hard_deadline = final_hard;
        job.penalty_per_sec = final_penalty;
        
        job.state = JobState::Assigned; // Move to next state

        // E. Save
        env.storage().persistent().set(&DataKey::Job(job_id), &job);
    }

    // STEP 3: ACCEPT OFFER (Freelancer agrees to the deal)
    // ----------------------------------------------------------------
    pub fn accept_job(env: Env, job_id: u64) {
        let mut job: Job = env.storage().persistent().get(&DataKey::Job(job_id)).expect("Job not found");

        // A. Logic: Must be Assigned
        if job.state != JobState::Assigned {
            panic!("Job is not currently assigned to anyone");
        }

        // B. Security: Only the assigned Freelancer can accept
        let freelancer_addr = job.freelancer.clone().expect("No freelancer data");
        freelancer_addr.require_auth();

        // C. Update State to 'Accepted'
        // This locks the deal. Next step is Funding.
        job.state = JobState::Accepted;

        // D. Save
        env.storage().persistent().set(&DataKey::Job(job_id), &job);
    }
    // ----------------------------------------------------------------
    // OPTIONAL: UPDATE JOB (Edit details before anyone accepts)
    // ----------------------------------------------------------------
    pub fn update_job(
        env: Env,
        job_id: u64,
        new_amount: i128,
        new_soft: u64,
        new_hard: u64,
        new_penalty: i128
    ) {
        // A. Load Job
        let mut job: Job = env.storage().persistent()
            .get(&DataKey::Job(job_id))
            .expect("Job not found");

        // B. Security: Only Client can edit
        job.client.require_auth();

        // C. Logic: VITAL CHECK!
        // You cannot edit if a freelancer is already assigned or money is locked.
        if job.state != JobState::Open {
            panic!("Cannot edit job: It is already assigned or in progress.");
        }

        // D. Overwrite the Data
        job.amount = new_amount;
        job.soft_deadline = new_soft;
        job.hard_deadline = new_hard;
        job.penalty_per_sec = new_penalty;

        // E. Save (This replaces the old data in the blockchain's memory)
        env.storage().persistent().set(&DataKey::Job(job_id), &job);
    }
    // ----------------------------------------------------------------
    // OPTIONAL: CANCEL JOB (Remove it and refund if needed)
    // ----------------------------------------------------------------
    /*pub fn cancel_job(env: Env, job_id: u64) {
        let mut job: Job = env.storage().persistent()
            .get(&DataKey::Job(job_id))
            .expect("Job not found");

        // A. Security
        job.client.require_auth();

        // B. Logic: Refund Check
        // If money is already inside, we MUST send it back before closing.
        if job.state == JobState::Funded {
            let token_client = token::Client::new(&env, &job.token);
            token_client.transfer(
                &env.current_contract_address(), // From Contract
                &job.client,                     // Back to Client
                &job.amount,                     // Full Amount
            );
        }

        // C. Logic: Cannot cancel if work is already submitted
        if job.state == JobState::Completed {
            panic!("Cannot cancel: Job is already completed.");
        }

        // D. "Delete" logic
        // we set the state to 'Failed' so no one can interact with it.
        job.state = JobState::Failed; 
        
        // E. Save
        env.storage().persistent().set(&DataKey::Job(job_id), &job);
    }*/
}
