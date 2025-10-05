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

if (typeof encodeURIComponent !== "function") {
    encodeURIComponent = function (s) {
        return escape(s);
    };
}