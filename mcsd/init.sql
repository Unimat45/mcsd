CREATE TABLE Servers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    directory TEXT NOT NULL,
    max_ram INTEGER DEFAULT 1024,
    min_ram INTEGER DEFAULT 512,
    version TEXT DEFAULT "latest"
);

CREATE UNIQUE INDEX idx_server_title ON Servers(title);
CREATE INDEX idx_server_version ON Servers(version);
