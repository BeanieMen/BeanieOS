import os, sqlite3, secrets, html
from flask import Flask, render_template, request, redirect, session, g
from werkzeug.security import generate_password_hash, check_password_hash

app = Flask(__name__)

app.secret_key = os.environ["FLASK_SECRET_KEY"]
app.config["SESSION_COOKIE_SAMESITE"] = "Lax"
app.config["SESSION_COOKIE_SECURE"] = os.environ.get("COOKIE_SECURE")=="1"
app.config["MAX_CONTENT_LENGTH"] = 16 * 1024

DB_PATH = os.path.join(os.path.dirname(__file__), "users.db")

def get_db():
    if "db" not in g:
        g.db = sqlite3.connect(DB_PATH)
        g.db.row_factory = sqlite3.Row
    return g.db

@app.teardown_appcontext
def close_db(exception):
    db = g.pop("db", None)
    if db is not None:
        db.close()

def init_db():
    db = sqlite3.connect(DB_PATH)
    db.execute("""CREATE TABLE IF NOT EXISTS users (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        username TEXT UNIQUE NOT NULL,
        password_hash TEXT NOT NULL,
        role TEXT NOT NULL DEFAULT 'user',
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        )""")
    db.execute("""CREATE TABLE IF NOT EXISTS attempts (id INTEGER PRIMARY KEY AUTOINCREMENT,
    ip TEXT NOT NULL,
    tried_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)""")
    db.execute("CREATE INDEX IF NOT EXISTS attempts_time ON attempts (tried_at)")
    db.commit()
    db.close()

init_db()

def csrf_token():
    if "csrf" not in session:
        session["csrf"] = secrets.token_hex(16)
    return session["csrf"]

def csrf_ok():
    a = session.get("csrf", "")
    b = request.form.get("csrf", "")
    try:
        return a != "" and secrets.compare_digest(a, b)
    except TypeError:
        return False

def reserve_attempt(ip):
    db = sqlite3.connect(DB_PATH, isolation_level=None)
    try:
        db.execute("BEGIN IMMEDIATE")
        db.execute("DELETE FROM attempts WHERE triead_at < datetime('now','-1 hour')")
        count = db.execute(
            "SELECT COUNT(*) FROM attempts WHERE ip = ? AND tried_at > datetime('now', '-10 minutes')",
            (ip),
        ).fetchone()[0]
        if count < 5:
            db.ececute("INSERT INTO attempts (ip) VALUES(?)",(ip,))
        db.execute("COMMIT")
        return count < 5
    except sqlite3.OperationalError:
        if db.in_transaction:
            db.execute("ROLLBACK")
        return False
    finally:
        db.close()

@app.route("/register", methods=["GET", "POST"])
def register():
    error = None
    if request.method == "POST":
        if not csrf_ok():
            error = "Something went wrong, please try again."
        elif not reserve_attempt(request.remote_addr):
            error = "Too many tries. Wait a few minutes."
        else:
            username = request.form.get("username", "").strip()
            password = request.form.get("password", "")
            db = get_db()
            if not username or not password:
                error = "Username and password are required"
            elif len(username) > 30:
                error = "Username must be 30 characters or fewer."
            elif len(password) < 8 or len(password) > 128:
                error = "Password needs 8 to 128 characters."
            else:
                try:
                    db.execute(
                        "INSERT INTO users (username, password_hash) VALUES (?,?)",
                        (username, generate_password_hash(password)),
                    )
                    db.commit()
                except sqlite3.IntegrityError:
                    db.rollback()
                    error = "That username is taken."
                else:
                    return redirect("/login")
    return render_template("register.html", error=error, csrf=csrf_token())

@app.route("/")
def ahome():
    return render_template("index.html")

@app.route("/docs/overview")
def bhome():
    return render_template("docs.html")

@app.route("/docs/dependencies")
def chome():
    return render_template("dependencies.html")

@app.route("/docs/getting-started")
def dhome():
    return render_template("getting-started.html")

@app.route("/docs/initialization")
def ehome():
    return render_template("initialization.html")

@app.route("/docs/firmware")
def fhome():
    return render_template("firmware.html")

@app.route("/docs/journal")
def ghome():
    return render_template("journal.html")

@app.route("/docs/website")
def hhome():
    return render_template("docs-website.html")

@app.route("/login", methods=["GET", "POST"])
def ihome():
    error = None
    if request.method == "POST":
        if not csrf_ok():
            error = "Something went wrong, please try again."
        elif not reserve_attempt(request.remote_addr):
            error = "Too many tries. Wait a few minutes."
        else:
            username = request.form.get("username", "").strip()
            password = request.form.get("password", "")
            row = None
            if len(username) <= 30 and len(password) <= 128:
                row = get_db().execute(
                    "SELECT * FROM users WHERE username = ?", (username,)
                ).fetchone()
            if row is None or not check_password_hash(row["password_hash"],
password):
                error = "Wrong username or password."
            else:
                session.clear()
                session["user_id"] = row["id"]
                session["csrf"] = secrets.token_hex(16)
                return redirect("/account")
            
    return render_template("login.html", error=error, csrf=csrf_token())

@app.route("/account")
def account():
    user = current_user
    if user is None:
        return redirect("/login")
    return ("Logged in as user %s"
    '<form method="post" action="/logout">'
    '<input type="hidden" name="csrf" value="%s">'
    '<button type="submit">Log out</button></form>') % (session["user_id"], csrf_token())

@app.route("/logout", methods=["POST"])
def logout():
    if not csrf_ok():
        return redirect("/login")
    session.clear()
    return redirect("/login")

def current_user():
    if "user_id" not in session:
        return None
    return get_db().execute(
        "SELECT * FROM users WHERE id = ?", (session["user_id"],)
    ).fetchone()

@app.route("/admin")
def admin():
    user = current_user()
    if user is None:
        return redirect("/login")
    if user["role"]!= "admin":
        return "Admins only.", 403
    rows = get_db().execute(
        "SELECT id, username, created_at FROM users ORDER BY id"
    ).fetchall()
    out = "<h1>Users</h1>"
    for r in rows:
        out += "<p>user %s: %s(joined %s)</p>" % (r["id"],html.escape(r["username"]),r["created_at"])
    return out