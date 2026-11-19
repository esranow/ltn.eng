FROM busybox
RUN mkdir /www && echo v1 > /www/index.html
EXPOSE 8080
CMD ["httpd","-f","-v","-p","8080","-h","/www"]
