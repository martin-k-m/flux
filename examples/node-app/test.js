const assert = require('assert')
const { greeting } = require('./index')

assert.ok(greeting().includes('Flux'), 'greeting should mention Flux')
console.log('ok - greeting mentions Flux')
