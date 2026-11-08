ALTER TABLE definitions ADD COLUMN agent_profile JSON CHECK(agent_profile IS NULL OR json_valid(agent_profile));
PRAGMA user_version=14;
