"""Offline regression tests for documentation links and standalone inventories."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest


SPEC = importlib.util.spec_from_file_location(
    'docs_checker', Path(__file__).resolve().parents[2] / 'scripts/check/check-docs.py'
)
docs = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(docs)


class DocumentationChecks(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        original = docs.ROOT
        docs.ROOT = self.root
        self.addCleanup(setattr, docs, 'ROOT', original)
        self.crate = self.root / 'crates/example'
        self.source = self.crate / 'src/lib.rs'
        self.source.parent.mkdir(parents=True)
        self.source.write_text('pub fn entry() {}\n')
        self.index = self.root / 'docs/crates/devdesk-example/files.md'
        self.index.parent.mkdir(parents=True)
        self.index.write_text(
            '# Files\n\nBefore\n' + docs.START + '\n' + docs.END + '\nAfter\n'
        )

    def test_missing_index_reports_a_diagnostic(self):
        self.index.unlink()
        self.assertIn('missing crate file index', docs.check_inventory(self.crate)[0])

    def test_invalid_markers_are_not_rewritten(self):
        for body in (docs.END + docs.START, docs.START + docs.START + docs.END, ''):
            with self.subTest(body=body):
                self.index.write_text(body)
                self.assertIn('invalid', docs.check_inventory(self.crate, True)[0])
                self.assertEqual(self.index.read_text(), body)

    def test_write_preserves_prose_and_is_idempotent(self):
        self.assertIn('stale', docs.check_inventory(self.crate)[0])
        self.assertEqual(docs.check_inventory(self.crate, True), [])
        result = self.index.read_text()
        self.assertTrue(result.startswith('# Files\n\nBefore\n'))
        self.assertTrue(result.endswith('\nAfter\n'))
        self.assertIn('[src/lib.rs]', result)
        self.assertEqual(docs.check_inventory(self.crate), [])
        docs.check_inventory(self.crate, True)
        self.assertEqual(self.index.read_text(), result)

    def test_addition_and_deletion_make_inventory_stale(self):
        docs.check_inventory(self.crate, True)
        extra = self.crate / 'src/other.rs'
        extra.write_text('pub struct Other;\n')
        self.assertTrue(docs.check_inventory(self.crate))
        docs.check_inventory(self.crate, True)
        extra.unlink()
        self.assertTrue(docs.check_inventory(self.crate))

    def test_curated_topic_link_is_preserved(self):
        self.index.write_text(
            docs.START + '\n| [src/lib.rs](../../../crates/example/src/lib.rs) '
            '| [流程](flow.md) |\n' + docs.END
        )
        docs.check_inventory(self.crate, True)
        self.assertIn('[流程](flow.md)', self.index.read_text())

    def test_binary_asset_is_indexed_without_decoding(self):
        (self.crate / 'asset.bin').write_bytes(b'\xff\xfe\x00')
        docs.check_inventory(self.crate, True)
        self.assertIn('[asset.bin]', self.index.read_text())

    def test_git_includes_new_sources_but_ignores_local_outputs(self):
        subprocess.run(['git', 'init', '-q', str(self.root)], check=True)
        (self.root / '.gitignore').write_text('crates/example/build/\n')
        output = self.crate / 'build/output.bin'
        output.parent.mkdir()
        output.write_bytes(b'\xff')
        self.assertEqual(docs.source_files(self.crate), [self.source])
        # An intentional tracked asset must remain covered even if ignored later.
        subprocess.run(['git', '-C', str(self.root), 'add', '-f', str(output)], check=True)
        self.assertEqual(set(docs.source_files(self.crate)), {self.source, output})

    def test_missing_document_file_and_anchor_are_reported(self):
        page = self.index.parent / 'overview.md'
        target = self.index.parent / 'topic.md'
        target.write_text('# 标题\n\n## 重复\n\n## 重复\n')
        page.write_text('[ok](topic.md#标题) [ok2](topic.md#重复-1) '
                        '[bad](topic.md#不存在) [missing](missing.md) '
                        '[external](https://example.invalid/unused)')
        errors = docs.check_links([page, self.index.parent / 'absent.md'])
        self.assertEqual(len(errors), 3)
        self.assertTrue(any('missing heading' in error for error in errors))
        self.assertTrue(any('missing link target' in error for error in errors))
        self.assertTrue(any('missing document' in error for error in errors))


if __name__ == '__main__':
    unittest.main()
