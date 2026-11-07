-- Unknown historical repository routing cannot be reconstructed from mutable Project settings.
ALTER TABLE attempts ADD COLUMN git_context JSON CHECK(git_context IS NULL OR json_valid(git_context));
ALTER TABLE advances ADD COLUMN context JSON CHECK(context IS NULL OR json_valid(context));
PRAGMA user_version=13;
