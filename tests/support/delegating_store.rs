//! Native port forwarding for storage-boundary fault injection fixtures.
macro_rules! delegated_native_store {
    () => {
        fn namespace(&self) -> devdesk_core::concurrency::StoreNamespace {
            self.inner.namespace()
        }
        fn read_resources(
            &self,
            _keys: &[devdesk_core::concurrency::ResourceKey],
        ) -> Result<Vec<devdesk_core::concurrency::ResourceSnapshot>> {
            self.inner.read_resources(_keys)
        }
        fn replay_commit(
            &self,
            _command_id: &str,
            _fingerprint: &str,
        ) -> Result<Option<devdesk_core::concurrency::CommitReceipt>> {
            self.inner.replay_commit(_command_id, _fingerprint)
        }
        fn orchestration_summary(
            &self,
            _query: &devdesk_core::read_models::ControllerQuery,
        ) -> Result<devdesk_core::read_models::OrchestrationSummary> {
            self.inner.orchestration_summary(_query)
        }
        fn workflow_driver_summary(
            &self,
            _id: &str,
        ) -> Result<devdesk_core::read_models::WorkflowDriverSummary> {
            self.inner.workflow_driver_summary(_id)
        }
        fn workflow_decision(
            &self,
            _id: &str,
        ) -> Result<devdesk_core::read_models::DecisionDetail> {
            self.inner.workflow_decision(_id)
        }
        fn workflow_management_summary(
            &self,
            _id: &str,
            _child_offset: u64,
        ) -> Result<devdesk_core::read_models::WorkflowManagementSummary> {
            self.inner.workflow_management_summary(_id, _child_offset)
        }
        fn workflow_progress(
            &self,
            _id: &str,
            _stage_offset: u64,
        ) -> Result<devdesk_core::read_models::WorkflowProgress> {
            self.inner.workflow_progress(_id, _stage_offset)
        }
        fn workflow_detail(
            &self,
            _query: &devdesk_core::read_models::WorkflowDetailQuery,
        ) -> Result<Option<devdesk_core::read_models::WorkflowDetailFacts>> {
            self.inner.workflow_detail(_query)
        }
        fn execution_task_detail(&self, _turn: &str) -> Result<Option<Value>> {
            self.inner.execution_task_detail(_turn)
        }
        fn project_records(&self, _method: &str, _params: &Value) -> Result<Value> {
            self.inner.project_records(_method, _params)
        }
        fn conversation_query(&self, _method: &str, _params: &Value) -> Result<Value> {
            self.inner.conversation_query(_method, _params)
        }
        fn workflow_reading(
            &self,
            _view: devdesk_core::read_models::WorkflowReadingView,
            _params: &Value,
        ) -> Result<Value> {
            self.inner.workflow_reading(_view, _params)
        }
        fn experience_headers(
            &self,
            _after: Option<&str>,
        ) -> Result<devdesk_core::read_models::ExperienceHeaderPage> {
            self.inner.experience_headers(_after)
        }
        fn experience_directory_source(
            &self,
            _id: &str,
        ) -> Result<Option<devdesk_core::read_models::ExperienceDirectorySource>> {
            self.inner.experience_directory_source(_id)
        }
        fn repository_registration_facts(
            &self,
        ) -> Result<devdesk_core::repository_updates::RepositoryRegistrationFacts> {
            self.inner.repository_registration_facts()
        }
        fn repository_refresh_facts(
            &self,
        ) -> Result<devdesk_core::repository_updates::RepositoryRefreshFacts> {
            self.inner.repository_refresh_facts()
        }
        fn repository_connect_projection_facts(
            &self,
            _operation: &str,
        ) -> Result<devdesk_core::repository_projection::RepositoryConnectProjectionFacts> {
            self.inner.repository_connect_projection_facts(_operation)
        }
        fn project_metadata(&self) -> Result<devdesk_domain::Project> {
            self.inner.project_metadata()
        }
        fn workspace_reading(
            &self,
            params: &serde_json::Value,
        ) -> devdesk_domain::Result<serde_json::Value> {
            self.inner.workspace_reading(params)
        }
        fn execution_workspace(
            &self,
            _query: &devdesk_core::read_models::ExecutionWorkspaceQuery,
        ) -> Result<devdesk_core::read_models::ExecutionWorkspace> {
            self.inner.execution_workspace(_query)
        }
        fn observe_query(
            &self,
            _stage: &'static str,
        ) -> Box<dyn devdesk_core::ports::QueryObservation> {
            self.inner.observe_query(_stage)
        }
        fn active_conversation_count(&self) -> Result<usize> {
            self.inner.active_conversation_count()
        }
        fn scheduling_hints(&self) -> Result<devdesk_core::read_models::SchedulingHints> {
            self.inner.scheduling_hints()
        }
        fn scheduling_hints_at(
            &self,
            clock: &devdesk_domain::ExecutionClockAnchor,
        ) -> Result<devdesk_core::read_models::SchedulingHints> {
            self.inner.scheduling_hints_at(clock)
        }
        fn has_pending_work(&self) -> Result<bool> {
            self.inner.has_pending_work()
        }
        fn event_cursor(&self) -> Result<u64> {
            self.inner.event_cursor()
        }
        fn event_hints_after(
            &self,
            cursor: u64,
            limit: usize,
        ) -> Result<Vec<devdesk_core::read_models::TailEvent>> {
            self.inner.event_hints_after(cursor, limit)
        }
        fn events_after(&self, cursor: u64, limit: usize) -> Result<Vec<devdesk_domain::Event>> {
            self.inner.events_after(cursor, limit)
        }
        fn subscribe_changes(&self, _listener: std::sync::Weak<dyn Fn() + Send + Sync>) {
            self.inner.subscribe_changes(_listener)
        }
        fn has_pending_tool_calls(&self) -> Result<bool> {
            self.inner.has_pending_tool_calls()
        }
        fn encode_runtime_record(
            &self,
            _record: &devdesk_domain::ConfigurationRecord,
        ) -> Result<Value> {
            self.inner.encode_runtime_record(_record)
        }
        fn encode_configuration_record(
            &self,
            _catalog: &devdesk_core::ConfigurationCatalogSnapshot,
            _record: &devdesk_domain::ConfigurationRecord,
        ) -> Result<Value> {
            self.inner.encode_configuration_record(_catalog, _record)
        }
        fn encode_configuration_value(
            &self,
            _catalog: &devdesk_core::ConfigurationCatalogSnapshot,
            _value: &Value,
        ) -> Result<Value> {
            self.inner.encode_configuration_value(_catalog, _value)
        }
        fn restore_runtime_value(&self, _value: &Value) -> Result<Value> {
            self.inner.restore_runtime_value(_value)
        }
        fn encode_runtime_value(&self, _value: &Value) -> Result<Value> {
            self.inner.encode_runtime_value(_value)
        }
        fn encode_execution_task(
            &self,
            _task: &devdesk_domain::ConfiguredAgentTask,
        ) -> Result<(Value, String)> {
            self.inner.encode_execution_task(_task)
        }
        fn encode_workflow_input_intent(
            &self,
            _record: &devdesk_domain::ConfigurationRecord,
        ) -> Result<Value> {
            self.inner.encode_workflow_input_intent(_record)
        }
        fn provider_tool_intent_page(
            &self,
            _after: Option<&str>,
        ) -> Result<devdesk_core::tool_calls::ToolCallPage> {
            self.inner.provider_tool_intent_page(_after)
        }
        fn provider_poll_page(
            &self,
            _after: Option<&str>,
        ) -> Result<devdesk_core::tool_calls::ToolCallPage> {
            self.inner.provider_poll_page(_after)
        }
        fn conversation_capture_page(
            &self,
            _after: Option<&str>,
        ) -> Result<devdesk_core::tool_calls::ToolCallPage> {
            self.inner.conversation_capture_page(_after)
        }
        fn conversation_capture_facts(
            &self,
            _session: &Session,
            _turn: &str,
            _terminal: &str,
            _worker_boundary: u64,
        ) -> Result<devdesk_core::ConversationCaptureFacts> {
            self.inner
                .conversation_capture_facts(_session, _turn, _terminal, _worker_boundary)
        }
        fn conversation_input_facts(
            &self,
            _session: &Session,
            through_turn: Option<&str>,
        ) -> Result<devdesk_core::ConversationCaptureFacts> {
            self.inner.conversation_input_facts(_session, through_turn)
        }
        fn conversation_page(
            &self,
            _queued: bool,
            _after: Option<&str>,
        ) -> Result<devdesk_core::tool_calls::ToolCallPage> {
            self.inner.conversation_page(_queued, _after)
        }
        fn conversation_runtime_facts(
            &self,
            _turn: &str,
        ) -> Result<Option<devdesk_core::ConversationRuntimeFacts>> {
            self.inner.conversation_runtime_facts(_turn)
        }
        fn conversation_answers_pending(
            &self,
            _turn: &str,
        ) -> Result<Vec<devdesk_domain::PendingInteraction>> {
            self.inner.conversation_answers_pending(_turn)
        }
        fn conversation_start_source(
            &self,
            _turn: &str,
        ) -> Result<devdesk_core::ConversationStartSource> {
            self.inner.conversation_start_source(_turn)
        }
        fn context_call_page(
            &self,
            _after: Option<&str>,
        ) -> Result<devdesk_core::tool_calls::ToolCallPage> {
            self.inner.context_call_page(_after)
        }
        fn context_call_facts(&self, _id: &str) -> Result<devdesk_core::ContextCallFacts> {
            self.inner.context_call_facts(_id)
        }
        fn platform_calls_pending(&self) -> Result<bool> {
            self.inner.platform_calls_pending()
        }
        fn runtime_record_page(
            &self,
            _kind: &str,
            _status: &str,
            _after: Option<&str>,
        ) -> Result<devdesk_core::tool_calls::ToolCallPage> {
            self.inner.runtime_record_page(_kind, _status, _after)
        }
        fn runtime_record_page_at(
            &self,
            kind: &str,
            status: &str,
            after: Option<&str>,
            clock: &devdesk_domain::ExecutionClockAnchor,
        ) -> Result<devdesk_core::tool_calls::ToolCallPage> {
            self.inner
                .runtime_record_page_at(kind, status, after, clock)
        }
        fn restore_runtime_record(&self, _value: &Value) -> Result<Value> {
            self.inner.restore_runtime_record(_value)
        }
        fn workflow_intent_page(
            &self,
            _after: Option<&str>,
        ) -> Result<devdesk_core::WorkflowIntentPage> {
            self.inner.workflow_intent_page(_after)
        }
        fn workflow_intent_facts(
            &self,
            _id: &str,
            _runnable_only: bool,
        ) -> Result<Option<devdesk_core::WorkflowIntentFacts>> {
            self.inner.workflow_intent_facts(_id, _runnable_only)
        }
        fn workflow_intent_active(&self, _id: &str, _revision: Option<u64>) -> Result<bool> {
            self.inner.workflow_intent_active(_id, _revision)
        }
        fn workflow_tool_facts(
            &self,
            _authority: &devdesk_core::tool_calls::WorkflowToolAuthority,
            _project: &str,
        ) -> Result<devdesk_core::tool_calls::WorkflowToolFacts> {
            self.inner.workflow_tool_facts(_authority, _project)
        }
        fn catalog_revision(&self) -> Result<u64> {
            self.inner.catalog_revision()
        }
        fn configuration_snapshot(
            &self,
        ) -> Result<Arc<devdesk_core::ConfigurationCatalogSnapshot>> {
            self.inner.configuration_snapshot()
        }
        fn project_id(&self) -> Option<String> {
            self.inner.project_id()
        }
    };
}
