if (!window.JSON) {
    window.JSON = {
        parse: function (s) {
            return eval("(" + s + ")");
        },
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

if (!String.prototype.startsWith) {
    Object.defineProperty(String.prototype, "startsWith", {
        value: function (search, rawPos) {
            var pos = rawPos > 0 ? rawPos | 0 : 0;
            return this.substring(pos, pos + search.length) === search;
        },
    });
}
