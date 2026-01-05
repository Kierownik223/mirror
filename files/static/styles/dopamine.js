window.addEventListener('DOMContentLoaded', function () {
    var sidebardiv = document.createElement("div");
    sidebardiv.setAttribute('class', 'dp-sbdiv dp-sbdivhid');
    document.querySelectorAll('img').forEach(img => {
        if (img.src.includes('/static/images/icons/favicon.png') || img.src.includes('/static/images/icons/hires/favicon.png')) {
            img.remove();
        }
    });

    var dpcat;
    var xhr = typeof XMLHttpRequest != 'undefined' ? new XMLHttpRequest() : new ActiveXObject('Microsoft.XMLHTTP');
    xhr.open('get', '/static/styles/dopaminesidebar.html', true);
    xhr.onreadystatechange = function () {
        if (xhr.readyState == 4 && xhr.status == 200) {
            sidebardiv.innerHTML = xhr.responseText;
        }
        sidebardiv.querySelector("#togglesidebar").addEventListener('click', function () {
            sidebardiv.classList.toggle("dp-sbdivhid")
        })
        dpcat = sidebardiv.querySelector("#dp-sb-cat")
        function createTree(container, foldername) {
            fetch(`/api/listing${foldername}`, { credentials: "same-origin" })
                .then(res => res.json())
                .then(data => {
                    const ul = document.createElement('ul');

                    data.forEach(item => {
                        const li = document.createElement('li');

                        li.textContent += item.name;
                        if (item.ext === 'folder') {
                            li.className = 'folder';
                            li.addEventListener('click', function (e) {
                                e.stopPropagation();
                                if (li.classList.contains('open')) {
                                    li.classList.remove('open');
                                    li.querySelectorAll('ul').forEach(child => child.remove());
                                } else {
                                    li.classList.add('open');
                                    createTree(li, foldername + item.name + '/');
                                }
                            });
                        }
                        else if (["png", "jpeg", "jpg", "svg"].includes(item.ext)) {
                            li.className = 'img';
                            li.addEventListener('click', function (e) {
                                e.stopPropagation();
                                if (li.classList.contains('open')) {
                                    li.classList.remove('open');
                                    li.querySelectorAll('ul').forEach(child => child.remove());
                                } else {
                                    li.classList.add('open');
                                    createTree(li, foldername + item.name + '/');
                                }
                            });
                        } else if (["gif", "mp4", "3gp"].includes(item.ext)) {
                            li.className = 'anm';
                            li.addEventListener('click', function (e) {
                                e.stopPropagation();
                                if (li.classList.contains('open')) {
                                    li.classList.remove('open');
                                    li.querySelectorAll('ul').forEach(child => child.remove());
                                } else {
                                    li.classList.add('open');
                                    createTree(li, foldername + item.name + '/');
                                }
                            });
                        } else if (["php", "webp", "java", "jar"].includes(item.ext)) {
                            li.className = 'shp';
                            li.addEventListener('click', function (e) {
                                e.stopPropagation();
                                if (li.classList.contains('open')) {
                                    li.classList.remove('open');
                                    li.querySelectorAll('ul').forEach(child => child.remove());
                                } else {
                                    li.classList.add('open');
                                    createTree(li, foldername + item.name + '/');
                                }
                            });
                        } else if (["mp3", "wav", "m4a"].includes(item.ext)) {
                            li.className = 'snd';
                            li.addEventListener('click', function (e) {
                                e.stopPropagation();
                                if (li.classList.contains('open')) {
                                    li.classList.remove('open');
                                    li.querySelectorAll('ul').forEach(child => child.remove());
                                } else {
                                    li.classList.add('open');
                                    createTree(li, foldername + item.name + '/');
                                }
                            });
                        } else if (["zip", "7z"].includes(item.ext)) {
                            li.className = 'zpi';
                            li.addEventListener('click', function (e) {
                                e.stopPropagation();
                                if (li.classList.contains('open')) {
                                    li.classList.remove('open');
                                    li.querySelectorAll('ul').forEach(child => child.remove());
                                } else {
                                    li.classList.add('open');
                                    createTree(li, foldername + item.name + '/');
                                }
                            });
                        } else {
                            li.className = 'file';
                            li.addEventListener('click', function (e) {
                                e.stopPropagation();
                                window.location.href = '/' + item.name;
                            });
                        }

                        ul.appendChild(li);
                    });

                    container.appendChild(ul);
                })
                .catch(err => console.error(err));
        }

        createTree(dpcat, '/');
    }
    xhr.send();
    sidebardiv.setAttribute("id", "sidebardp")
    this.document.body.appendChild(sidebardiv)
});
