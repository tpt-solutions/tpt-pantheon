//! Distributed solver cluster for tpt-telos.
//!
//! Provides a worker pool that dispatches [`VerificationProblem`]s to multiple
//! solver workers for parallel verification. The initial implementation uses
//! an in-process thread pool; a gRPC transport can be added behind a `grpc`
//! feature flag in the future.
//!
//! Usage:
//! ```
//! use tpt_telos_verifier::cluster::SolverPool;
//! use tpt_telos_ir::VerificationProblem;
//!
//! let pool = SolverPool::new(4); // 4 worker threads
//! let problems: Vec<VerificationProblem> = vec![];
//! let results = pool.verify_all(&problems);
//! ```

use std::sync::{mpsc, Arc};
use std::thread;

use tpt_telos_ir::VerificationProblem;

use crate::verify::{verify, VerificationResult};

/// A solver worker that processes verification problems.
///
/// Workers are stateless: they receive a problem, run the solver, and return
/// the result.
pub trait SolverWorker: Send + Sync {
    /// Verify a single problem and return the result.
    fn verify(&self, problem: &VerificationProblem) -> VerificationResult;
}

/// The default worker that uses the built-in Fourier-Motzkin solver.
pub struct FourierMotzkinWorker;

impl SolverWorker for FourierMotzkinWorker {
    fn verify(&self, problem: &VerificationProblem) -> VerificationResult {
        verify(problem)
    }
}

/// An in-process worker pool that dispatches problems to worker threads.
///
/// Each worker runs on its own thread and processes problems independently.
/// Results are collected in order.
pub struct SolverPool {
    workers: Vec<mpsc::Sender<(usize, VerificationProblem)>>,
    result_rx: mpsc::Receiver<(usize, VerificationResult)>,
    _handles: Vec<thread::JoinHandle<()>>,
}

impl SolverPool {
    /// Create a new worker pool with the given number of worker threads.
    ///
    /// Each worker uses the built-in Fourier-Motzkin solver.
    pub fn new(num_workers: usize) -> Self {
        Self::with_worker(FourierMotzkinWorker, num_workers)
    }

    /// Create a new worker pool with a custom worker implementation.
    pub fn with_worker<W: SolverWorker + 'static>(worker: W, num_workers: usize) -> Self {
        let shared_worker: Arc<dyn SolverWorker> = Arc::new(worker);
        let (result_tx, result_rx) = mpsc::channel();
        let mut workers = Vec::new();
        let mut handles = Vec::new();

        for _ in 0..num_workers {
            let (task_tx, task_rx) = mpsc::channel::<(usize, VerificationProblem)>();
            let result_tx = result_tx.clone();
            let w = WorkerWrapper {
                worker: shared_worker.clone(),
            };
            let handle = thread::spawn(move || {
                while let Ok((idx, problem)) = task_rx.recv() {
                    let result = w.worker.verify(&problem);
                    if result_tx.send((idx, result)).is_err() {
                        break;
                    }
                }
            });
            workers.push(task_tx);
            handles.push(handle);
        }

        SolverPool {
            workers,
            result_rx,
            _handles: handles,
        }
    }

    /// Verify all problems in parallel and return results in order.
    pub fn verify_all(&self, problems: &[VerificationProblem]) -> Vec<VerificationResult> {
        let n = problems.len();
        if n == 0 {
            return Vec::new();
        }

        // Send all problems to workers.
        for (idx, problem) in problems.iter().enumerate() {
            let worker_idx = idx % self.workers.len();
            let _ = self.workers[worker_idx].send((idx, problem.clone()));
        }

        // Collect results in order.
        let mut results: Vec<Option<VerificationResult>> = vec![None; n];
        let mut received = 0;
        while received < n {
            if let Ok((idx, result)) = self.result_rx.recv() {
                results[idx] = Some(result);
                received += 1;
            }
        }

        results.into_iter().map(|r| r.unwrap()).collect()
    }

    /// Return the number of workers in the pool.
    pub fn num_workers(&self) -> usize {
        self.workers.len()
    }
}

struct WorkerWrapper {
    worker: Arc<dyn SolverWorker>,
}

// FourierMotzkinWorker needs to be cloneable for the pool.
impl Clone for FourierMotzkinWorker {
    fn clone(&self) -> Self {
        FourierMotzkinWorker
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tpt_telos_ir::{Constraint, Linear, Relation};

    #[test]
    fn pool_verifies_single_problem() {
        let pool = SolverPool::new(2);
        let problem = VerificationProblem {
            func_name: "test".to_string(),
            func_span: Default::default(),
            premises: vec![Constraint(
                Linear::var("x").sub(&Linear::constant_only(0)),
                Relation::Ge,
            )],
            conclusions: vec![],
        };
        let results = pool.verify_all(&[problem]);
        assert_eq!(results.len(), 1);
        assert!(results[0].all_passed);
    }

    #[test]
    fn pool_verifies_multiple_problems_in_parallel() {
        let pool = SolverPool::new(4);
        let problems: Vec<VerificationProblem> = (0..10)
            .map(|i| VerificationProblem {
                func_name: format!("f{}", i),
                func_span: Default::default(),
                premises: vec![Constraint(
                    Linear::var("x").sub(&Linear::constant_only(0)),
                    Relation::Ge,
                )],
                conclusions: vec![],
            })
            .collect();
        let results = pool.verify_all(&problems);
        assert_eq!(results.len(), 10);
        for r in &results {
            assert!(r.all_passed);
        }
    }

    #[test]
    fn pool_preserves_order() {
        let pool = SolverPool::new(2);
        let problems: Vec<VerificationProblem> = (0..6)
            .map(|i| VerificationProblem {
                func_name: format!("f{}", i),
                func_span: Default::default(),
                premises: vec![],
                conclusions: vec![],
            })
            .collect();
        let results = pool.verify_all(&problems);
        assert_eq!(results.len(), 6);
        for (i, r) in results.iter().enumerate() {
            assert_eq!(r.func_name, format!("f{}", i));
        }
    }
}
