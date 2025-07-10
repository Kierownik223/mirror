if (!window.JSON) {
    window.JSON = {
        parse: function (s) {
            return eval('(' + s + ')');
        }
    };
}

if (typeof decodeURIComponent !== "function") {
    decodeURIComponent = function (s) {
        return unescape(s);
    };
}