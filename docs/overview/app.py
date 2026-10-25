import os, sqlite3, secrets
from flask import Flask, render_template, request, redirect, session, g
from werkzeug.security import generate_password_hash, check_password_hash

app = Flask(__name__)

app.secret_key = os.environ["FLASK_SECRET_KEY"]
app.config["SESSION_COOKIE_SAMESITE"] = "Lax"
app.config["SESSION_COOKIE_SECURE"] = os.environ.get("COOKIE_SECURE")=="1"

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
        password_harsh TEXT NOT NULL,
        role TEXT NOT NULL DEFAULT 'user',
        created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
    )""")
    db.commit()
    db.close()

init_db()

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

@app.route("/login")
def ihome():
    return render_template("login.html")