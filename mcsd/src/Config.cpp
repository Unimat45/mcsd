#include "Config.hpp"
#include <cstdarg>
#include <cstdio>
#include <sqlite3.h>
#include <cstring>

#define cmp(a,b) (strcmp(a, b) == 0)

constexpr const char *db_name = "dev.db";

static int res_callback(void *list, int argc, char **argv, char **azColName) {
    auto res = reinterpret_cast<std::vector<std::shared_ptr<Config>>*>(list);

    int id, min_ram, max_ram;
    std::string title, directory, version;

    for (int i = 0; i < argc; i++) {
        if (cmp(azColName[i], "id")) {
           id = std::stoi(argv[i]); 
        }
        else if (cmp(azColName[i], "title")) {
            title = std::string(argv[i]);
        }
        else if (cmp(azColName[i], "directory")) {
            directory = std::string(argv[i]);
        }
        else if (cmp(azColName[i], "version")) {
            version = std::string(argv[i]);
        }
        else if (cmp(azColName[i], "max_ram")) {
            max_ram = std::stoi(argv[i]);
        }
        else if (cmp(azColName[i], "min_ram")) {
            min_ram = std::stoi(argv[i]);
        }
        else {
            return 1;
        }
    }

    std::shared_ptr<Config> cfg = std::shared_ptr<Config>(new Config(id, title, directory, version, min_ram, max_ram));
    res->push_back(cfg);

    return 0;
}

int Config::get_id() { return this->id; }
std::string Config::get_title() { return this->title; }
std::string Config::get_directory() { return this->directory; }
std::string Config::get_version() { return this->version; }
int Config::get_max_ram() { return this->max_ram; }
int Config::get_min_ram() { return this->min_ram; }

std::vector<std::shared_ptr<Config>> Config::query(const char *cmd, ...) {
    va_list args;

    va_start(args, cmd);
    std::string query;

    {
        char buf[1024];
        vsprintf(buf, cmd, args);
        query = std::string(buf);
    }

    va_end(args);

    std::vector<std::shared_ptr<Config>> res;

    sqlite3 *db;
    int rc = sqlite3_open(db_name, &db);
    if (rc != 0) {
        // TODO: Error
        sqlite3_close(db);
        throw;
    }

    char *zErrMsg = NULL;
    rc = sqlite3_exec(db, query.c_str(), res_callback, &res, &zErrMsg);

    if (rc != SQLITE_OK) {
        sqlite3_free(zErrMsg);
    }
    sqlite3_close(db);
    return res;
}

