from flask import Flask, render_template

app = Flask(__name__)
@app.route("/")
def whome():
    return render_template("index.html")

@app.route("/docs/overview")
def ghome():
    return render_template("docs.html")

@app.route("/docs/dependencies")
def chome():
    return render_template("dependencies.html")

@app.route("/docs/getting-started")
def bhome():
    return render_template("getting-started.html")

@app.route("/docs/initialization")
def ahome():
    return render_template("initialization.html")

@app.route("/docs/firmware")
def dhome():
    return render_template("firmware.html")

@app.route("/docs/journal")
def ehome():
    return render_template("journal.html")

@app.route("/docs/website")
def fhome():
    return render_template("docs-website.html")