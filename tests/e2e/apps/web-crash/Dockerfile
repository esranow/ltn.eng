FROM busybox
RUN mkdir /www && echo crash > /www/index.html
EXPOSE 8080
CMD ["sh","-c","echo boom; exit 3"]
