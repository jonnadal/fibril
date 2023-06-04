use std::num::NonZeroUsize;

use {
    consistency_model::{ConsistencyTester, SequentialSpec},
    fibril::Sdk,
    fibril_core::Id,
    std::fmt::Debug,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct RequestId(NonZeroUsize);

impl From<RequestId> for usize {
    fn from(req_id: RequestId) -> Self {
        req_id.0.get()
    }
}

impl From<usize> for RequestId {
    fn from(value: usize) -> Self {
        RequestId(NonZeroUsize::new(value).expect("Zero is an invalid request ID"))
    }
}

// This type is leaked by [`ConsistencyClient`] but not explicitly published.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ThreadId(usize);

impl From<ThreadId> for usize {
    fn from(req_id: ThreadId) -> Self {
        req_id.0
    }
}

impl From<usize> for ThreadId {
    fn from(value: usize) -> Self {
        ThreadId(value)
    }
}

pub trait Synchronous<RefObj>
where
    RefObj: SequentialSpec,
{
    fn encode_request(req_id: RequestId, op: &RefObj::Op) -> Self;
    fn decode_response(self) -> (RequestId, RefObj::Ret);
}

/// Provides an abstraction that a [`fibril::Fiber`] can use to validate whether a distributed
/// system obeys particular consistency semantics.
///
/// # Example
///
/// ```rust
/// use consistency_model::{LinearizabilityTester, Register, RegisterOp, RegisterRet};
/// use fibril::{Fiber, Sdk};
/// use fibril_verifier::{ConsistencyClient, RequestId, Synchronous, Verifier};
///
/// #[derive(Clone, Debug, PartialEq)]
/// enum Msg {
///     Read(RequestId),
///     ReadOk(RequestId, String),
///     Write(RequestId, String),
///     WriteOk(RequestId),
/// }
///
/// impl Synchronous<Register<String>> for Msg {
///     fn encode_request(req_id: RequestId, op: &RegisterOp<String>) -> Self {
///         match op {
///             RegisterOp::Read => Msg::Read(req_id),
///             RegisterOp::Write(value) => Msg::Write(req_id, value.clone()),
///         }
///     }
///     fn decode_response(self) -> (RequestId, RegisterRet<String>) {
///         match self {
///             Msg::ReadOk(req_id, value) => (req_id, RegisterRet::ReadOk(value)),
///             Msg::WriteOk(req_id) => (req_id, RegisterRet::WriteOk),
///             _ => unreachable!(),
///         }
///     }
/// }
///
/// Verifier::new(|cfg| {
///     let server = cfg.spawn(Fiber::new(|sdk| {
///         let mut value = String::new();
///         loop {
///             let (src, msg) = sdk.recv();
///             match msg {
///                 Msg::Read(req_id) => sdk.send(src, Msg::ReadOk(req_id, value.clone())),
///                 Msg::Write(req_id, new_value) => {
///                     value = new_value;
///                     sdk.send(src, Msg::WriteOk(req_id));
///                 }
///                 _ => unreachable!(),
///             }
///         }
///     }));
///     cfg.spawn(Fiber::new(move |sdk| {
///         let mut consistency = ConsistencyClient::new(
///             &sdk,
///             LinearizabilityTester::new(Register::new("ORIGINAL".to_string())));
///         consistency.invoke(server, RegisterOp::Write("LATEST".to_string()));
///         consistency.invoke(server, RegisterOp::Read);
///         consistency.await_response();
///         consistency.await_response();
///     }));
/// });
/// ```
pub struct ConsistencyClient<'a, M, T> {
    sdk: &'a fibril::Sdk<'a, M>,
    tester: T,
    next_req_id: RequestId,
    thread_idx_to_request_id: Vec<Option<RequestId>>,
}

const REQUEST_ID_SKIP: usize = 100;

impl<'a, M, T> ConsistencyClient<'a, M, T> {
    pub fn await_response<R>(&mut self)
    where
        T: ConsistencyTester<ThreadId, R>,
        R: SequentialSpec,
        M: Synchronous<R>,
    {
        let (_src, msg) = self.sdk.recv();
        let (req_id, ret) = msg.decode_response();
        let tidx = match self
            .thread_idx_to_request_id
            .iter()
            .enumerate()
            .find(|(_tidx, rid)| rid == &&Some(req_id))
        {
            None => panic!("Invalid request ID: {req_id:?}"),
            Some((tidx, _rid)) => tidx,
        };
        self.thread_idx_to_request_id[tidx] = None;
        self.tester.on_return(ThreadId(tidx), ret).unwrap();
        assert!(self.tester.is_consistent());
    }

    pub fn invoke<R>(&mut self, dst: Id, op: R::Op)
    where
        T: ConsistencyTester<ThreadId, R>,
        R: SequentialSpec,
        M: Synchronous<R>,
    {
        let msg = Synchronous::encode_request(self.next_req_id, &op);
        self.sdk.send(dst, msg);

        let tidx = match self
            .thread_idx_to_request_id
            .iter()
            .enumerate()
            .find(|(_tidx, rid)| rid.is_none())
        {
            Some((tidx, _rid)) => tidx,
            None => {
                let tidx = self.thread_idx_to_request_id.len();
                self.thread_idx_to_request_id.push(Some(self.next_req_id));
                tidx
            }
        };
        self.thread_idx_to_request_id[tidx] = Some(self.next_req_id);
        self.tester.on_invoke(ThreadId(tidx), op).unwrap();

        self.next_req_id.0 = self.next_req_id.0.checked_add(REQUEST_ID_SKIP).unwrap();
    }

    pub fn new(sdk: &'a Sdk<'a, M>, tester: T) -> Self {
        ConsistencyClient {
            sdk,
            tester,
            next_req_id: RequestId(NonZeroUsize::new(REQUEST_ID_SKIP).unwrap()),
            thread_idx_to_request_id: Vec::new(),
        }
    }
}
