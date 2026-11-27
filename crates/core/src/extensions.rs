//! Extension groups are kernel-provided capabilities that only some domains need.
//! A domain package binds its own handlers to the hooks a group declares.
use crate::{Result, ports, ports::DomainReader};
use devdesk_domain::{
    ProjectTestDiscovery, PullRequestCreation, PullRequestRequest, TestRunner, WorktreeKind, err,
    require,
};
use serde_json::{Value, json};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hook {
    pub name: &'static str,
    /// The kernel input a handler bound to this hook declares.
    pub core: Option<&'static str>,
}

/// Where the value of a provided workflow input comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProvidedSource {
    /// The test configuration selected for the project the workflow runs in.
    ProjectTestConfiguration,
}

/// A workflow input an extension group fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProvidedInput {
    pub name: &'static str,
    pub source: ProvidedSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtensionGroup {
    pub id: &'static str,
    pub version: u32,
    /// Platform tool handlers only a package requiring this group may call.
    pub handlers: &'static [&'static str],
    /// Kernel inputs only such a package may declare for its handlers.
    pub core_inputs: &'static [&'static str],
    pub hooks: &'static [Hook],
    /// Named file collections a package may publish for this group.
    pub resources: &'static [&'static str],
    /// Workflow inputs a workflow may declare this group the provider of.
    pub provided_inputs: &'static [ProvidedInput],
}

impl ExtensionGroup {
    pub fn reference(&self) -> String {
        format!("{}/{}", self.id, self.version)
    }

    pub fn hook(&self, name: &str) -> Option<&'static Hook> {
        self.hooks.iter().find(|hook| hook.name == name)
    }

    pub fn provided(&self, name: &str) -> Option<ProvidedSource> {
        self.provided_inputs
            .iter()
            .find(|input| input.name == name)
            .map(|input| input.source)
    }
}

pub const EXTENSION_GROUPS: &[ExtensionGroup] = &[
    ExtensionGroup {
        id: "engineering.git",
        version: 1,
        handlers: &[
            "workspace.current",
            "workspace.snapshot",
            "workspace.checkpoint",
            "workspace.target",
            "git.source.publish",
            "git.delivery.open",
            "git.delivery.merge",
            "git.candidate",
            "git.advance",
            "git.publish",
        ],
        core_inputs: &["engineering.git.head", "engineering.git.snapshot"],
        hooks: &[
            Hook {
                name: "worktree_branch",
                core: None,
            },
            Hook {
                name: "pull_request_creation",
                core: None,
            },
        ],
        resources: &[],
        provided_inputs: &[],
    },
    ExtensionGroup {
        id: "engineering.tests",
        version: 1,
        handlers: &["tests.run", "tests.project"],
        core_inputs: &[],
        hooks: &[Hook {
            name: "test_discovery",
            core: Some("engineering.git.snapshot"),
        }],
        resources: &["reporters"],
        provided_inputs: &[ProvidedInput {
            name: "test_configuration",
            source: ProvidedSource::ProjectTestConfiguration,
        }],
    },
];

pub fn extension_group(reference: &str) -> Option<&'static ExtensionGroup> {
    EXTENSION_GROUPS
        .iter()
        .find(|group| group.reference() == reference)
}

/// Kernel inputs every package may declare for its handlers, whatever extensions it requires.
/// An input of an extension group is listed with that group and needs the group to be required.
pub const KERNEL_INPUTS: &[&str] = &["domain.entities"];

/// Whether a kernel input of this name exists on this Host.
pub fn core_input_known(input: &str) -> bool {
    KERNEL_INPUTS.contains(&input) || group_of_core_input(input).is_some()
}

/// The group providing a platform tool handler; `None` for a kernel-wide handler.
pub fn group_of_handler(handler: &str) -> Option<&'static ExtensionGroup> {
    EXTENSION_GROUPS
        .iter()
        .find(|group| group.handlers.contains(&handler))
}

/// What a package that requires the Git extension means by a project connection: the
/// repository to register and the mainline to follow. It belongs with the engineering code
/// and leaves with it.
pub fn git_connection(connection: &Value) -> Result<(String, String)> {
    let field = |name: &str, missing: &str| {
        connection[name]
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| err("domain_contract", missing))
    };
    Ok((
        field("repository", "连接缺少仓库")?,
        field("mainline", "连接缺少目标分支")?,
    ))
}

/// How the group fills a workflow input it is declared to provide.
pub fn input_provider(group: &str, input: &str) -> Option<ProvidedSource> {
    extension_group(group)?.provided(input)
}

/// The group a kernel input belongs to; `None` for a kernel-wide input.
pub fn group_of_core_input(input: &str) -> Option<&'static ExtensionGroup> {
    EXTENSION_GROUPS
        .iter()
        .find(|group| group.core_inputs.contains(&input))
}

/// The groups engineering consults; packages decide which of their handlers to bind.
pub const ENGINEERING_GIT: &str = "engineering.git/1";
pub const ENGINEERING_TESTS: &str = "engineering.tests/1";

/// The branch a package names for a worktree of `owner`; `None` when it binds no policy.
pub(crate) fn worktree_branch(
    domain: &Arc<dyn DomainReader>,
    project: &str,
    owner: &str,
    kind: WorktreeKind,
) -> Result<Option<String>> {
    let Some(value) = domain.hook(
        project,
        ENGINEERING_GIT,
        "worktree_branch",
        json!({"owner": owner, "kind": kind}),
    )?
    else {
        return Ok(None);
    };
    require(
        value["owner"] == owner && value["kind"] == json!(kind),
        "领域策略改变了工作副本归属",
    )?;
    value["branch"]
        .as_str()
        .filter(|branch| {
            !branch.is_empty()
                && branch.len() <= 240
                && !branch.starts_with('-')
                && !branch.chars().any(char::is_control)
        })
        .map(|branch| Some(branch.to_owned()))
        .ok_or_else(|| err("invalid_domain_package", "领域策略返回了无效的分支"))
}

/// The title and body a package writes for a delivery pull request; `None` when it binds no policy.
pub(crate) fn pull_request_creation(
    domain: &Arc<dyn DomainReader>,
    request: &PullRequestRequest,
) -> Result<Option<PullRequestCreation>> {
    let Some(value) = domain.hook(
        &request.project,
        ENGINEERING_GIT,
        "pull_request_creation",
        json!({"request": request, "marker": request.marker()}),
    )?
    else {
        return Ok(None);
    };
    let creation: PullRequestCreation =
        serde_json::from_value(value).map_err(|error| err("domain_contract", error.to_string()))?;
    creation.validate(request)?;
    Ok(Some(creation))
}

/// The reporter a package publishes for a test runner; `None` when it publishes none.
pub(crate) fn test_reporter(
    domain: &Arc<dyn DomainReader>,
    project: &str,
    runner: &TestRunner,
) -> Result<Option<ports::TestReporter>> {
    let key = json!(runner);
    Ok(domain
        .resource(
            project,
            ENGINEERING_TESTS,
            "reporters",
            key.as_str().unwrap_or(""),
        )?
        .map(|(digest, content)| ports::TestReporter { digest, content }))
}

/// The test entries a package recognizes in a tracked Git snapshot; `None` when it binds no policy.
pub(crate) fn project_test_discovery(
    domain: &Arc<dyn DomainReader>,
    git: &dyn ports::Git,
    project: &str,
    mainline: &str,
) -> Result<Option<ProjectTestDiscovery>> {
    let Some((path, revision)) = git.registered_revision(project, mainline)? else {
        return Ok(domain
            .has_hook(project, ENGINEERING_TESTS, "test_discovery")?
            .then(ProjectTestDiscovery::default));
    };
    let Some(value) = domain.hook(
        project,
        ENGINEERING_TESTS,
        "test_discovery",
        json!({"path": path, "revision": revision}),
    )?
    else {
        return Ok(None);
    };
    let result: ProjectTestDiscovery =
        serde_json::from_value(value).map_err(|error| err("domain_contract", error.to_string()))?;
    require(
        result.revision.as_deref() == Some(revision.as_str()),
        "测试入口改变了原 Git 修订",
    )?;
    result.configuration.validate()?;
    Ok(Some(result))
}
