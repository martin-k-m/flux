project "python-app"
language python

pipeline {
    step dependencies {
        command "pip install -r requirements.txt"
    }
    step test {
        needs dependencies
        command "python -m unittest discover -p \"test_*.py\""
    }
}
