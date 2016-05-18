requirejs.config({
    paths: {
        'text': '../libraries/require/text',
        'durandal':'../libraries/durandal',
        'plugins' : '../libraries/durandal/plugins',
        'transitions' : '../libraries/durandal/transitions',
        'knockout': '../libraries/knockout/knockout',
        'jquery': '../libraries/jquery/jquery'
    }
});

define(function (require) {
    var system = require('durandal/system');
    var app = require('durandal/app');
    var viewLocator = require('durandal/viewLocator');

    system.debug(true);

    app.title = 'TapeDeck';

    app.configurePlugins({
        router: true,
        dialog: true
    });

    app.start().then(function() {
        viewLocator.useConvention();
        app.setRoot('viewmodels/shell');
    });
});
