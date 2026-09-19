function createBreadcrumbs(path) {
    var container = document.createElement("span");
    container.className = "breadcrumbs";
    container.id = "breadcrumbs";

    container.appendChild(document.createTextNode("/"));

    if (path && path !== "/") {
        var segments = path.split("/");
        var numSegments = segments.length;

        for (var i = 0; i < numSegments; i++) {
            var segment = segments[i];
            if (segment === "") continue;

            var subpathArr = segments.slice(0, i + 1);
            var subpath = subpathArr.join("/");
            
            var isShareSegment = segment === "share" && (i === 0 || i === 1);
            if (i === numSegments - 1 || isShareSegment) {
                var span = document.createElement("span");
                span.innerText = isShareSegment ? segment + "/" : segment;
                container.appendChild(span);
            } else {
                var span2 = document.createElement("span");
                var a = document.createElement("a");

                a.href = subpath + "/";
                a.innerText = segment;
                span2.appendChild(a);

                span2.appendChild(document.createTextNode("/"));

                container.appendChild(span2);
            }
        }
    }

    return container;
}
