use {
    consistency_model::{ConsistencyTester, SequentialSpec},
    fibril::Fiber,
    fibril_core::{Id, Step},
    std::{collections::VecDeque, fmt::Debug},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct RequestId(usize);

impl From<RequestId> for usize {
    fn from(req_id: RequestId) -> Self {
        req_id.0
    }
}

impl From<usize> for RequestId {
    fn from(value: usize) -> Self {
        RequestId(value)
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
    fn decode_response(&self) -> (RequestId, RefObj::Ret);
}

pub struct ConsistencyClient<Tester, RefObj>
where
    RefObj: SequentialSpec,
    Tester: ConsistencyTester<ThreadId, RefObj>,
{
    tester: Tester,
    threads: Vec<VecDeque<(Id, <RefObj as SequentialSpec>::Op)>>,
}

impl<Tester, RefObj> ConsistencyClient<Tester, RefObj>
where
    RefObj: SequentialSpec,
    Tester: ConsistencyTester<ThreadId, RefObj> + 'static,
{
    pub fn into_behavior<Msg>(mut self) -> impl Step<Msg>
    where
        Msg: Synchronous<RefObj> + 'static,
        RefObj: Clone + 'static,
        RefObj::Op: Clone + Debug + 'static,
        RefObj::Ret: Clone + Debug,
    {
        Fiber::<Msg>::new(move |sdk| {
            let mut thread_to_req_id = vec![None; self.threads.len()];
            let mut next_req_id = RequestId(100);
            for (thread_i, pairs) in self.threads.iter_mut().enumerate() {
                if let Some((dst, op)) = pairs.pop_front() {
                    thread_to_req_id[thread_i] = Some(next_req_id);
                    let msg = Msg::encode_request(next_req_id, &op);
                    self.tester.on_invoke(thread_i.into(), op).unwrap();
                    assert!(self.tester.is_consistent());
                    sdk.send(dst, msg);
                    next_req_id.0 += 100;
                }
            }
            loop {
                let (_src, msg) = sdk.recv();
                let (req_id, ret) = msg.decode_response();
                let thread_i = thread_to_req_id
                    .iter()
                    .enumerate()
                    .find(|(_i, ri)| **ri == Some(req_id))
                    .unwrap()
                    .0;
                self.tester.on_return(thread_i.into(), ret).unwrap();
                assert!(self.tester.is_consistent());
                if let Some((dst, op)) = self.threads[thread_i].pop_front() {
                    thread_to_req_id[thread_i] = Some(next_req_id);
                    let msg = Msg::encode_request(next_req_id, &op);
                    self.tester.on_invoke(thread_i.into(), op).unwrap();
                    assert!(self.tester.is_consistent());
                    sdk.send(dst, msg);
                    next_req_id.0 += 100;
                }
            }
        })
    }

    pub fn new(tester: Tester) -> Self {
        Self {
            tester,
            threads: Vec::new(),
        }
    }

    pub fn thread(mut self, definition: impl IntoIterator<Item = (Id, RefObj::Op)>) -> Self {
        self.threads.push(definition.into_iter().collect());
        self
    }
}
