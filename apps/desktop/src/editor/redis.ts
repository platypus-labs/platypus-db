// The editor's language for Redis: one command per line, arguments quoted as redis-cli
// quotes them, `#` and `--` comments. Completion of the commands at the start of a line and of
// the key types (virtual tables) after BROWSE.
import type { Completion, CompletionContext, CompletionResult } from "@codemirror/autocomplete";
import { LanguageSupport, StreamLanguage, type StringStream } from "@codemirror/language";
import type { CompletionCatalog } from "../api";

const command = (label: string, detail: string, info: string): Completion => ({ label, type: "keyword", detail, info });

const COMMANDS: Completion[] = [
  command("BROWSE", "[db<n>.]type [MATCH pattern] [LIMIT rows]", "The keys of one type as rows: string, hash, list, set, zset, json or stream."),
  command("GET", "key", "The value of a string key."),
  command("SET", "key value [EX seconds] [NX|XX] [KEEPTTL]", "Sets a string key."),
  command("MGET", "key [key …]", "The values of several string keys."),
  command("INCR", "key", "Adds 1 to a number."),
  command("DEL", "key [key …]", "Deletes keys."),
  command("UNLINK", "key [key …]", "Deletes keys in the background."),
  command("EXISTS", "key [key …]", "How many of the keys exist."),
  command("TYPE", "key", "The type of a key."),
  command("RENAME", "key newkey", "Renames a key."),
  command("EXPIRE", "key seconds", "Sets a key's time to live."),
  command("PERSIST", "key", "Removes a key's time to live."),
  command("TTL", "key", "The seconds a key has left; -1 without expiry."),
  command("SCAN", "cursor [MATCH pattern] [COUNT n] [TYPE type]", "Iterates the keys without blocking the server."),
  command("HGET", "key field", "A field of a hash."),
  command("HSET", "key field value [field value …]", "Sets fields of a hash."),
  command("HGETALL", "key", "The fields and values of a hash."),
  command("HDEL", "key field [field …]", "Deletes fields of a hash."),
  command("HSCAN", "key cursor [MATCH pattern] [COUNT n]", "Iterates the fields of a hash."),
  command("LRANGE", "key start stop", "Items of a list; 0 -1 for all."),
  command("LPUSH", "key value [value …]", "Adds items at the head of a list."),
  command("RPUSH", "key value [value …]", "Adds items at the tail of a list."),
  command("LSET", "key index value", "Sets the item at an index."),
  command("LREM", "key count value", "Removes items equal to value."),
  command("LLEN", "key", "The length of a list."),
  command("SMEMBERS", "key", "The members of a set."),
  command("SADD", "key member [member …]", "Adds members to a set."),
  command("SREM", "key member [member …]", "Removes members of a set."),
  command("SSCAN", "key cursor [MATCH pattern] [COUNT n]", "Iterates the members of a set."),
  command("ZRANGE", "key start stop [WITHSCORES]", "Members of a sorted set by rank."),
  command("ZADD", "key [NX|XX] score member [score member …]", "Adds members with their scores."),
  command("ZREM", "key member [member …]", "Removes members of a sorted set."),
  command("ZSCORE", "key member", "The score of a member."),
  command("XADD", "key id|* field value [field value …]", "Adds an entry to a stream."),
  command("XRANGE", "key start end [COUNT n]", "Entries of a stream; - + for all."),
  command("XDEL", "key id [id …]", "Deletes entries of a stream."),
  command("JSON.GET", "key [path …]", "A JSON document, or the values at paths ($.a)."),
  command("JSON.SET", "key path value [NX|XX]", "Sets a JSON document, or a value at a path."),
  command("JSON.DEL", "key [path]", "Deletes a JSON document or a value at a path."),
  command("SELECT", "db", "Moves the session to another database."),
  command("MULTI", "", "Starts a transaction: commands queue until EXEC."),
  command("EXEC", "", "Runs the queued commands as one transaction."),
  command("DISCARD", "", "Drops the queued commands."),
  command("DBSIZE", "", "The number of keys of the database."),
  command("INFO", "[section]", "The server's information and statistics."),
  command("PING", "", "Checks the connection."),
  command("FLUSHDB", "", "Deletes every key of the database."),
];

const TYPES = ["string", "hash", "list", "set", "zset", "json", "stream"];

/** The highlighting: the command, strings, numbers and comments. */
const tokens = StreamLanguage.define<{ first: boolean }>({
  name: "redis",
  startState: () => ({ first: true }),
  token(stream: StringStream, state) {
    if (stream.sol()) state.first = true;
    if (stream.eatSpace()) return null;
    if (state.first && (stream.match("#") || stream.match("--"))) {
      stream.skipToEnd();
      return "comment";
    }
    const quote = stream.peek();
    if (quote === '"' || quote === "'") {
      stream.next();
      let escaped = false;
      for (let c = stream.next(); c !== undefined; c = stream.next()) {
        if (c === quote && !escaped) break;
        escaped = !escaped && c === "\\";
      }
      state.first = false;
      return "string";
    }
    stream.eatWhile(/\S/);
    const word = stream.current();
    const first = state.first;
    state.first = false;
    if (first) return "keyword";
    return /^-?\d+(\.\d+)?$/.test(word) ? "number" : null;
  },
  languageData: { commentTokens: { line: "#" } },
});

function completeRedis(catalog: CompletionCatalog | null | undefined) {
  return (context: CompletionContext): CompletionResult | null => {
    const line = context.state.doc.lineAt(context.pos);
    const before = line.text.slice(0, context.pos - line.from);
    const word = context.matchBefore(/[\w.:-]*/);
    const from = word?.from ?? context.pos;
    // The command, at the start of the line.
    if (/^\s*[\w.]*$/.test(before)) {
      if (!context.explicit && word?.from === word?.to) return null;
      return { from, options: COMMANDS, validFor: /^[\w.]*$/ };
    }
    // A key type after BROWSE, with or without its database.
    if (/^\s*browse\s+[\w.]*$/i.test(before)) {
      const dbs = Object.keys(catalog?.schemas ?? {});
      const options: Completion[] = [
        ...TYPES.map((label): Completion => ({ label, type: "class" })),
        ...dbs.flatMap((db) => TYPES.map((type): Completion => ({ label: `${db}.${type}`, type: "class" }))),
      ];
      return { from, options, validFor: /^[\w.]*$/ };
    }
    return null;
  };
}

/** Redis highlighting and completion for a Redis editor. */
export function redisLanguage(catalog: CompletionCatalog | null | undefined) {
  return new LanguageSupport(tokens, [tokens.data.of({ autocomplete: completeRedis(catalog) })]);
}
