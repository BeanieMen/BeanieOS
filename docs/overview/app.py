import os, sqlite3, secrets
from flask import Flask, render_template, request, redirect, session, g

app = Flask(__name__)

app.secret_key = os.environ["FLASK_SECRET_KEY"]
app.config["SESSION_COOKIE_SAMESITE"] = "Lax"
app.config["SESSION_COOKIE_SECURE"] = os.environ.get("COOKIE_SECURE")=="1"

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