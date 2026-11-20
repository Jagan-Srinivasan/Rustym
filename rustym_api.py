"""Tiny YouTube Music client used by rustym (no third-party deps).

usage: rustym_api.py COOKIES.TXT MODE [ARG]
  MODE liked              -> {"tracks": [...]}
  MODE playlist ID        -> {"tracks": [...]}
  MODE playlists          -> {"playlists": [{"id","title"}...]}
  MODE verify             -> {"ok": true}   (is the saved session still signed in?)
  MODE diag               -> JSON health report (cookie NAMES only, never values)
Prints one JSON object; on failure {"error": "..."}.
Rotated cookies from YouTube's responses are written back to COOKIES.TXT (like a browser does).
Authenticates exactly like the music.youtube.com web app (cookies + SAPISIDHASH).
"""
import datetime
import hashlib
import http.cookies
import json
import os
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

ORIGIN = "https://music.youtube.com"
UA = "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0"


def die(msg):
    print(json.dumps({"error": str(msg)}))
    sys.exit(0)


def _is_yt(domain):
    d = domain.lstrip(".").lower()
    return d == "youtube.com" or d.endswith(".youtube.com")


def _is_google(domain):
    d = domain.lstrip(".").lower()
    return d == "google.com" or d.endswith(".google.com") or (d.startswith("google.") or ".google." in d)


class Jar:
    """Cookie records read from a Netscape cookies.txt, written back when YouTube rotates them."""

    def __init__(self, path):
        self.path = path
        self.recs = []  # dicts: domain path secure expiry name value httponly
        self.dirty = False
        try:
            with open(path, encoding="utf-8", errors="replace") as f:
                for line in f:
                    line = line.rstrip("\r\n")
                    http_only = False
                    if line.startswith("#HttpOnly_"):
                        line = line[len("#HttpOnly_"):]
                        http_only = True
                    elif line.startswith("#") or not line.strip():
                        continue
                    p = line.split("\t")
                    if len(p) < 7:
                        continue
                    if _is_yt(p[0]) or _is_google(p[0]):
                        self.recs.append({"domain": p[0], "path": p[2], "secure": p[3].upper() == "TRUE",
                                          "expiry": p[4], "name": p[5], "value": p[6], "httponly": http_only})
        except OSError as e:
            die("cannot read cookies file: %s" % e)

    def values(self):
        """name -> value as the browser would send them to music.youtube.com.
        youtube.com cookies win; google.com ones only fill in names youtube.com lacks."""
        out = {}
        for r in self.recs:
            if _is_google(r["domain"]) and not _is_yt(r["domain"]):
                out.setdefault(r["name"], r["value"])
        for r in self.recs:
            if _is_yt(r["domain"]):
                out[r["name"]] = r["value"]
        return out

    def names(self):
        return sorted({r["name"] for r in self.recs})

    def merge_set_cookie(self, headers):
        """Apply Set-Cookie headers of a response (YouTube rotates *PSIDCC / *PSIDTS this way)."""
        for raw in headers:
            try:
                sc = http.cookies.SimpleCookie()
                sc.load(raw)
            except Exception:
                continue
            for name, m in sc.items():
                domain = m["domain"] or ".youtube.com"
                if not _is_yt(domain):
                    continue
                gone = m["max-age"] == "0"
                exp = "0"
                if m["expires"]:
                    try:
                        import email.utils
                        exp_ts = int(email.utils.parsedate_to_datetime(m["expires"]).timestamp())
                        exp = str(exp_ts)
                        if exp_ts < time.time():
                            gone = True
                    except Exception:
                        pass
                if m["max-age"].isdigit() and m["max-age"] != "0":
                    exp = str(int(time.time()) + int(m["max-age"]))
                if m.value == "" :
                    gone = True
                hit = [r for r in self.recs if r["name"] == name and _is_yt(r["domain"])]
                if gone:
                    if hit:
                        self.recs = [r for r in self.recs if r not in hit]
                        self.dirty = True
                    continue
                if hit:
                    for r in hit:
                        if r["value"] != m.value:
                            r["value"] = m.value
                            self.dirty = True
                        if exp != "0":
                            r["expiry"] = exp
                else:
                    self.recs.append({"domain": domain if domain.startswith(".") else "." + domain,
                                      "path": m["path"] or "/", "secure": bool(m["secure"]),
                                      "expiry": exp, "name": name, "value": m.value,
                                      "httponly": bool(m["httponly"])})
                    self.dirty = True

    def save(self):
        if not self.dirty:
            return
        lines = ["# Netscape HTTP Cookie File"]
        for r in self.recs:
            dom = r["domain"]
            lines.append("%s%s\t%s\t%s\t%s\t%s\t%s\t%s" % (
                "#HttpOnly_" if r["httponly"] else "", dom, "TRUE" if dom.startswith(".") else "FALSE",
                r["path"], "TRUE" if r["secure"] else "FALSE", r["expiry"], r["name"], r["value"]))
        tmp = "%s.%d.tmp" % (self.path, os.getpid())
        try:
            fd = os.open(tmp, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
            with os.fdopen(fd, "w", encoding="utf-8") as f:
                f.write("\n".join(lines) + "\n")
            os.replace(tmp, self.path)
            self.dirty = False
        except OSError:
            try:
                os.unlink(tmp)
            except OSError:
                pass


def sapisid_header(vals, ts):
    """Same Authorization header the web app sends: SAPISIDHASH (+ 1P / 3P variants when present)."""
    parts = []
    for scheme, key in (("SAPISIDHASH", "SAPISID"), ("SAPISID1PHASH", "__Secure-1PAPISID"),
                        ("SAPISID3PHASH", "__Secure-3PAPISID")):
        v = vals.get(key)
        if not v and scheme == "SAPISIDHASH":
            v = vals.get("__Secure-3PAPISID")
        if v:
            h = hashlib.sha1(("%s %s %s" % (ts, v, ORIGIN)).encode()).hexdigest()
            parts.append("%s %s_%s" % (scheme, ts, h))
    return " ".join(parts)


LAST = {"status": None}


def call(jar, extra_query, extra_body, endpoint="browse"):
    vals = jar.values()
    if not (vals.get("SAPISID") or vals.get("__Secure-3PAPISID")):
        die("no SAPISID cookie - please sign in again")
    ts = str(int(time.time()))
    cookie = "; ".join("%s=%s" % kv for kv in vals.items())
    if "SOCS" not in vals:
        cookie += "; SOCS=CAI"
    day = datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(days=1)
    ver = "1." + day.strftime("%Y%m%d") + ".01.00"
    body = {"context": {"client": {"clientName": "WEB_REMIX", "clientVersion": ver, "hl": "en", "gl": "IN"}}}
    body.update(extra_body)
    req = urllib.request.Request(
        ORIGIN + "/youtubei/v1/" + endpoint + "?prettyPrint=false" + extra_query,
        data=json.dumps(body).encode(),
        headers={
            "User-Agent": UA,
            "Accept": "*/*",
            "Content-Type": "application/json",
            "Authorization": sapisid_header(vals, ts),
            "X-Goog-AuthUser": "0",
            "X-Origin": ORIGIN,
            "Origin": ORIGIN,
            "Referer": ORIGIN + "/",
            "X-Youtube-Client-Name": "67",
            "X-Youtube-Client-Version": ver,
            "Cookie": cookie,
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=25) as r:
            LAST["status"] = getattr(r, "status", 200)
            try:
                jar.merge_set_cookie(r.headers.get_all("Set-Cookie") or [])
                jar.save()
            except Exception:
                pass
            return json.loads(r.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        LAST["status"] = e.code
        snippet = e.read()[:160].decode("utf-8", "replace").replace("\n", " ")
        if e.code in (401, 403):
            die("YouTube rejected the saved session (HTTP %d) - please sign in again" % e.code)
        die("YouTube Music replied HTTP %d (%s)" % (e.code, snippet))
    except Exception as ex:  # network down, DNS, timeout ...
        die("network error: %s" % ex)


def logged_in(resp):
    """True/False from the response's tracking params, None when YouTube doesn't say."""
    try:
        for p in resp["responseContext"]["serviceTrackingParams"]:
            for kv in p.get("params", []):
                if kv.get("key") == "logged_in":
                    return str(kv.get("value")) == "1"
    except Exception:
        pass
    return None


def need_login(resp, what):
    if logged_in(resp) is False:
        die("YouTube says this session is signed out (%s) - please sign in again" % what)


def walk(o):
    if isinstance(o, dict):
        yield o
        for v in o.values():
            yield from walk(v)
    elif isinstance(o, list):
        for v in o:
            yield from walk(v)


def runs_text(node):
    try:
        return "".join(r.get("text", "") for r in node["runs"])
    except Exception:
        return ""


def parse_dur(t):
    try:
        sec = 0
        for part in t.strip().split(":"):
            sec = sec * 60 + int(part)
        return float(sec)
    except Exception:
        return None


def tracks_in(resp):
    out = []
    for d in walk(resp):
        r = d.get("musicResponsiveListItemRenderer")
        if not isinstance(r, dict):
            continue
        if "GREY_OUT" in str(r.get("musicItemRendererDisplayPolicy", "")):
            continue  # unavailable song
        vid = (r.get("playlistItemData") or {}).get("videoId")
        if not vid:
            for x in walk(r):
                if isinstance(x.get("videoId"), str):
                    vid = x["videoId"]
                    break
        if not vid:
            continue
        cols = r.get("flexColumns") or []

        def col_runs(i):
            try:
                return cols[i]["musicResponsiveListItemFlexColumnRenderer"]["text"].get("runs", [])
            except Exception:
                return []

        title = "".join(x.get("text", "") for x in col_runs(0))
        artists = []
        for run in col_runs(1):
            be = (run.get("navigationEndpoint") or {}).get("browseEndpoint") or {}
            if str(be.get("browseId", "")).startswith("UC"):
                artists.append(run.get("text", ""))
        if not artists:
            sub = "".join(x.get("text", "") for x in col_runs(1))
            artists = [sub.split(" • ")[0]] if sub else []
        dur = None
        try:
            dur = parse_dur(runs_text(r["fixedColumns"][0]["musicResponsiveListItemFixedColumnRenderer"]["text"]))
        except Exception:
            pass
        if title:
            out.append({"id": vid, "title": title, "artist": ", ".join(a for a in artists if a), "dur": dur})
    return out


def next_token(resp):
    for d in walk(resp):
        n = d.get("nextContinuationData")
        if isinstance(n, dict) and n.get("continuation"):
            return n["continuation"]
        c = d.get("continuationCommand")
        if isinstance(c, dict) and c.get("token"):
            return c["token"]
    return None


def fetch_tracks(jar, browse_id, limit=300):
    resp = call(jar, "", {"browseId": browse_id})
    tracks = tracks_in(resp)
    if not tracks:
        need_login(resp, "empty library response")
    for _ in range(10):
        if len(tracks) >= limit:
            break
        tok = next_token(resp)
        if not tok:
            break
        q = urllib.parse.quote(tok, safe="")
        resp = call(jar, "&ctoken=%s&continuation=%s&type=next" % (q, q), {})
        more = tracks_in(resp)
        if not more:
            break
        tracks += more
    seen, uniq = set(), []
    for t in tracks:
        if t["id"] not in seen:
            seen.add(t["id"])
            uniq.append(t)
    return uniq


def playlists_in(resp):
    out, seen = [], set()
    for d in walk(resp):
        r = d.get("musicTwoRowItemRenderer")
        if not isinstance(r, dict):
            continue
        be = (r.get("navigationEndpoint") or {}).get("browseEndpoint") or {}
        bid = be.get("browseId")
        if not bid or bid in ("VLLM", "VLSE") or bid in seen:
            continue
        if not (bid.startswith("VL") or bid.startswith("MPRE")):
            continue  # artists, etc.
        title = runs_text(r.get("title") or {})
        if not title:
            continue
        seen.add(bid)
        out.append({"id": bid[2:] if bid.startswith("VL") else bid, "title": title})
    return out


def fetch_playlists(jar):
    resp = call(jar, "", {"browseId": "FEmusic_liked_playlists"})
    out = playlists_in(resp)
    if not out:
        need_login(resp, "empty playlists response")
    for _ in range(5):
        tok = next_token(resp)
        if not tok:
            break
        q = urllib.parse.quote(tok, safe="")
        resp = call(jar, "&ctoken=%s&continuation=%s&type=next" % (q, q), {})
        more = [p for p in playlists_in(resp) if p["id"] not in {x["id"] for x in out}]
        if not more:
            break
        out += more
    return out


def diag(jar):
    vals = jar.values()
    rep = {
        "cookie_names": jar.names(),
        "has_SAPISID": "SAPISID" in vals,
        "has___Secure-3PAPISID": "__Secure-3PAPISID" in vals,
        "has_youtube_domain_login": any(_is_yt(r["domain"]) and r["name"] in ("SAPISID", "__Secure-3PAPISID") for r in jar.recs),
        "has___Secure-1PSIDTS": "__Secure-1PSIDTS" in vals,
    }
    try:
        resp = call(jar, "", {"browseId": "FEmusic_liked_playlists"})
        rep["http_status"] = LAST["status"]
        rep["logged_in_flag"] = logged_in(resp)
        rep["playlists_found"] = len(playlists_in(resp))
        rep["liked_found"] = len(tracks_in(call(jar, "", {"browseId": "VLLM"})))
    except SystemExit:
        raise
    print(json.dumps(rep, indent=2))


def main():
    if len(sys.argv) < 3:
        die("usage: rustym_api.py COOKIES MODE [ARG]")
    jar = Jar(sys.argv[1])
    mode = sys.argv[2]
    arg = sys.argv[3] if len(sys.argv) > 3 else ""
    if mode == "liked":
        print(json.dumps({"tracks": fetch_tracks(jar, "VLLM")}))
    elif mode == "playlist":
        bid = arg if arg.startswith(("VL", "MPRE")) else "VL" + arg
        print(json.dumps({"tracks": fetch_tracks(jar, bid)}))
    elif mode == "playlists":
        print(json.dumps({"playlists": fetch_playlists(jar)}))
    elif mode == "verify":
        resp = call(jar, "", {"browseId": "FEmusic_liked_playlists"})
        need_login(resp, "verify")
        if logged_in(resp) is None and not playlists_in(resp):
            die("YouTube rejected the saved session (no library in reply) - please sign in again")
        print(json.dumps({"ok": True}))
    elif mode == "diag":
        diag(jar)
    else:
        die("unknown mode " + mode)


if __name__ == "__main__":
    main()
