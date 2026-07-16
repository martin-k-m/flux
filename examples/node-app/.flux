project "node-app"
language node

pipeline {
    step dependencies {
        command "npm install"
    }
    step test {
        needs dependencies
        command "npm test"
    }
}
