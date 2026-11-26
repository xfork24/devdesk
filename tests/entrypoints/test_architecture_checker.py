"""Offline regression tests for the kernel/domain boundary of the architecture checker."""
import importlib.util
from pathlib import Path
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location(
    'architecture_checker',
    Path(__file__).resolve().parents[2] / 'scripts/check/check-architecture.py'
)
architecture = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(architecture)


NAMESPACE = '"software-development.role-"'
RETIRED = '"builtin-role-reviewer"'
SHIM = 'crates/kernel/src/shim.rs'


class KernelDomainBoundary(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        self.write('plugins/core/manifest.json', '{"types": {"role": [], "tool": ["tools/t.json"]}}')
        (self.root / 'plugins/software-development').mkdir(parents=True)
        self.write('crates/kernel/src/lib.rs', 'pub fn entry() {}\n')
        self.write('crates/adapters/build.rs', 'fn main() {}\n')
        known = architecture.KNOWN_DOMAIN_REFERENCES
        architecture.KNOWN_DOMAIN_REFERENCES = {}
        self.addCleanup(setattr, architecture, 'KNOWN_DOMAIN_REFERENCES', known)

    def write(self, relative, text):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        return path

    def test_production_code_may_not_name_a_domain_package(self):
        self.write(SHIM, f'pub const A: &str = {NAMESPACE};\n')
        self.write('crates/kernel/src/paths.rs',
                   'pub const B: &str = "../../../plugins/software-development/runtime/worker.mjs";\n')
        errors = architecture.kernel_domain_errors(self.root)
        self.assertEqual(errors, [
            'crates/kernel/src/paths.rs:1 references a domain package path',
            f'{SHIM}:1 names a domain package namespace',
        ])

    def test_integration_tests_and_test_only_modules_are_not_production(self):
        self.write('crates/kernel/tests/flow.rs', f'pub const A: &str = {NAMESPACE}{RETIRED};\n')
        self.write('crates/kernel/src/lib.rs', '''pub fn entry() {}
#[cfg(test)]
#[path = "probe.rs"]
mod probe;
#[cfg(test)]
mod tests {
    fn probe() { let _ = "builtin-role-inline"; }
}
''')
        self.write('crates/kernel/src/probe.rs', f'''pub const A: &str = {RETIRED};
#[path = "nested.rs"]
mod nested;
''')
        self.write('crates/kernel/src/nested.rs', f'pub const A: &str = {NAMESPACE};\n')
        self.assertEqual(architecture.kernel_domain_errors(self.root), [])

    def test_a_module_reached_without_cfg_test_remains_production(self):
        self.write('crates/kernel/src/lib.rs', 'mod probe;\n')
        self.write('crates/kernel/src/probe.rs', f'pub const A: &str = {RETIRED};\n')
        self.assertEqual(architecture.kernel_domain_errors(self.root),
                         ['crates/kernel/src/probe.rs:1 names a retired factory identity'])

    def test_retired_identities_follow_the_factory_manifest(self):
        self.write(SHIM, f'pub const A: &str = {RETIRED};\n')
        self.assertEqual(architecture.kernel_domain_errors(self.root),
                         [f'{SHIM}:1 names a retired factory identity'])
        self.write('plugins/core/manifest.json',
                   '{"types": {"role": ["roles/reviewer.json"], "tool": []}}')
        self.write('crates/kernel/src/tool.rs', 'pub const A: &str = "builtin-tool-review:1";\n')
        self.assertEqual(architecture.kernel_domain_errors(self.root), [])

    def test_only_the_factory_loader_names_the_factory_directory(self):
        self.write('crates/adapters/examples/export.rs',
                   'fn main() { let _ = "../../plugins/core/manifest.json"; }\n')
        self.assertEqual(architecture.kernel_domain_errors(self.root),
                         ['crates/adapters/examples/export.rs:1 names the factory directory'])
        self.write('crates/adapters/examples/export.rs', 'fn main() {}\n')
        self.write('crates/adapters/build.rs', 'fn main() { let _ = "../../plugins"; }\n')
        self.assertEqual(architecture.kernel_domain_errors(self.root), [])

    def test_known_references_cover_exactly_what_remains(self):
        architecture.KNOWN_DOMAIN_REFERENCES = {
            SHIM: ('A1 legacy roles', ('names a retired factory identity',)),
        }
        self.write(SHIM, f'pub const A: &str = {RETIRED};\n')
        self.assertEqual(architecture.kernel_domain_errors(self.root), [])

        self.write(SHIM, f'pub const A: &str = {NAMESPACE}{RETIRED};\n')
        self.assertEqual(architecture.kernel_domain_errors(self.root),
                         [f'{SHIM}:1 names a domain package namespace'])

        self.write(SHIM, f'pub const A: &str = {NAMESPACE};\n')
        self.assertEqual(architecture.kernel_domain_errors(self.root), [
            f'{SHIM}:1 names a domain package namespace',
            f'known domain reference is stale: {SHIM} (names a retired factory identity)',
        ])

        self.write(SHIM, 'pub const A: &str = "builtin-model-x:1";\n')
        self.assertEqual(architecture.kernel_domain_errors(self.root), [
            f'known domain reference is stale: {SHIM} (names a retired factory identity)',
        ])


if __name__ == '__main__':
    unittest.main()
