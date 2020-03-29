  function body_onload(){
    var albums  = window.albums;
    var loading = {};
    var albimgs = {};

    var imgloaded = function(){
      if( loading[this.alb].length > 1 ){
        loading[this.alb].pop(this.src);
      }
      else{
        var cnv = document.getElementById('cnv_' + this.alb);
        cnv.style.display = "block";
        var ctx = cnv.getContext('2d');
        if( albimgs[this.alb].length > 0 ){
          ctx.rotate( -10. / 365. * 2 * Math.PI );
          ctx.translate(0, 75);
          ctx.drawImage(albimgs[this.alb][0], 0, 0);
          ctx.setTransform(1, 0, 0, 1, 0, 0);
        }

        if( albimgs[this.alb].length > 1 ){
          ctx.translate(150, 50);
          ctx.drawImage(albimgs[this.alb][1], 0, 0);
          ctx.setTransform(1, 0, 0, 1, 0, 0);
        }

        if( albimgs[this.alb].length > 2 ){
          ctx.rotate( 10. / 365. * 2 * Math.PI );
          ctx.translate(300, -25);
          ctx.drawImage(albimgs[this.alb][2], 0, 0);
          ctx.setTransform(1, 0, 0, 1, 0, 0);
        }
      }
    };

    for(var alb in albums){
      if(albums.hasOwnProperty(alb)){
        albimgs[alb] = [];
        loading[alb] = [];
        for(var i = 0; i < albums[alb].length; i++){
          var img = new Image();
          img.src = "/_/thumb/" + alb + "/" + albums[alb][i];
          img.alb = alb;
          img.onload = imgloaded;
          loading[alb].push(img.src);
          albimgs[alb].push(img);
        }
      }
    }
  };
